#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use base64::Engine;
use mosaic_core::{Bitmap, GridSelection, Mode};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::Mutex,
};
use tauri::{Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use zeroize::Zeroizing;

const LIMIT: u64 = 64 * 1024 * 1024;
struct Document {
    bytes: Vec<u8>,
    sidecar: Option<Vec<u8>>,
    name: String,
    block_origin: Option<Vec<u8>>,
    block_selection: Option<GridSelection>,
    prepared: Option<Box<PreparedDocument>>,
}
struct PreparedDocument {
    document: Document,
    report: mosaic_core::EncryptionPreview,
}
#[derive(Default)]
struct Session(Mutex<Option<Document>>);
#[derive(Serialize)]
struct View {
    name: String,
    width: u32,
    height: u32,
    bpp: u16,
    source_bpp: u16,
    mode: Option<Mode>,
    preview_url: String,
    preview_width: u32,
    preview_height: u32,
    selection: Option<GridSelection>,
}
fn view(d: &Document) -> Result<View, String> {
    let i = mosaic_core::inspect(&d.bytes, d.sidecar.as_deref()).map_err(|e| e.to_string())?;
    let w = i.width;
    let h = i.height;
    let mut png_bytes = vec![];
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&i.rgba)
            .map_err(|e| e.to_string())?;
    }
    let preview_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png_bytes)
    );
    Ok(View {
        name: d.name.clone(),
        width: i.width,
        height: i.height,
        bpp: i.bpp,
        source_bpp: i.source_bpp,
        mode: i.mode,
        preview_url,
        preview_width: w,
        preview_height: h,
        selection: d.block_selection.clone().or(i.selection),
    })
}
fn read(path: &PathBuf) -> Result<Vec<u8>, String> {
    let f = fs::File::open(path).map_err(|e| e.to_string())?;
    if f.metadata().map_err(|e| e.to_string())?.len() > LIMIT {
        return Err("文件超过 64 MiB 限制".into());
    }
    let mut bytes = vec![];
    f.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("文件过大".into());
    }
    Ok(bytes)
}
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn open_document(app: tauri::AppHandle) -> Result<Option<View>, String> {
    blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("BMP 图片", &["bmp"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        let bytes = read(&path)?;
        Bitmap::parse(bytes.clone()).map_err(|e| e.to_string())?;
        let companion = PathBuf::from(format!("{}.lmm", path.display()));
        let sidecar = if companion.exists() {
            Some(read(&companion)?)
        } else {
            None
        };
        let d = Document {
            prepared: None,
            block_origin: None,
            block_selection: None,
            bytes,
            sidecar,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        };
        let v = view(&d)?;
        let state: State<Session> = app.state();
        *state.0.lock().map_err(|e| e.to_string())? = Some(d);
        Ok(Some(v))
    })
    .await
}
#[tauri::command]
async fn attach_sidecar(app: tauri::AppHandle) -> Result<Option<View>, String> {
    blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("还原信息", &["lmm"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let data = read(&file.into_path().map_err(|e| e.to_string())?)?;
        let state: State<Session> = app.state();
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        let d = guard.as_mut().ok_or("请先打开 BMP")?;
        let candidate = Document {
            prepared: None,
            block_origin: None,
            block_selection: None,
            bytes: d.bytes.clone(),
            sidecar: Some(data),
            name: d.name.clone(),
        };
        let v = view(&candidate)?;
        *d = candidate;
        Ok(Some(v))
    })
    .await
}
#[tauri::command]
async fn transform(
    app: tauri::AppHandle,
    password: String,
    mode: Mode,
    selection: GridSelection,
    decrypt: bool,
    allow_alpha_loss: bool,
    existing_selection: Option<GridSelection>,
) -> Result<View, String> {
    let password = Zeroizing::new(password);
    blocking(move || {
        let state: State<Session> = app.state();
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        let d = guard.as_mut().ok_or("请先打开 BMP")?;
        d.prepared = None;
        let current = Bitmap::parse(d.bytes.clone()).map_err(|e| e.to_string())?;
        selection
            .validate(current.width, current.height)
            .map_err(|e| e.to_string())?;
        let info =
            mosaic_core::inspect(&d.bytes, d.sidecar.as_deref()).map_err(|e| e.to_string())?;
        let candidate = if decrypt {
            if mode == Mode::Entropy && info.mode.is_some() {
                return Err("这张图片有还原信息，请使用对应模式还原".into());
            }
            let remaining = if mode == Mode::Entropy {
                d.block_selection
                    .as_ref()
                    .map(|known| known.without(&selection, current.width, current.height))
                    .transpose()
                    .map_err(|e| e.to_string())?
            } else {
                None
            };
            let bytes = if mode == Mode::Entropy {
                if selection.is_empty() {
                    return Err("请先涂抹要还原的网格".into());
                }
                mosaic_core::transform_grid(&d.bytes, &password, &selection)
            } else {
                mosaic_core::restore(&d.bytes, d.sidecar.as_deref(), &password)
            }
            .map_err(|e| e.to_string())?;
            Document {
                bytes,
                sidecar: None,
                name: format!("{}-restored.bmp", d.name.trim_end_matches(".bmp")),
                prepared: None,
                block_origin: if remaining.as_ref().is_some_and(|s| !s.is_empty()) {
                    d.block_origin.clone()
                } else {
                    None
                },
                block_selection: remaining.filter(|s| !s.is_empty()),
            }
        } else {
            protect_document(
                d,
                &password,
                mode,
                &selection,
                allow_alpha_loss,
                existing_selection.as_ref(),
            )?
        };
        let v = view(&candidate)?;
        *d = candidate;
        Ok(v)
    })
    .await
}
fn protect_document(
    d: &Document,
    password: &str,
    mode: Mode,
    selection: &GridSelection,
    allow_alpha_loss: bool,
    existing_selection: Option<&GridSelection>,
) -> Result<Document, String> {
    let info = mosaic_core::inspect(&d.bytes, d.sidecar.as_deref()).map_err(|e| e.to_string())?;
    // Edit a target mask against the recovered source, so overlapping strokes never double-XOR.
    let base = if info.mode.is_some() {
        mosaic_core::restore(&d.bytes, d.sidecar.as_deref(), password).map_err(|e| e.to_string())?
    } else {
        if let Some(origin) = &d.block_origin {
            origin.clone()
        } else if let Some(existing) = &existing_selection {
            mosaic_core::transform_grid(&d.bytes, password, existing).map_err(|e| e.to_string())?
        } else {
            d.bytes.clone()
        }
    };
    let source = Bitmap::parse(base.clone()).map_err(|e| e.to_string())?;
    if mode == Mode::Alpha && source.bpp == 32 && !allow_alpha_loss {
        return Err("α 模式会覆盖原有透明度，请勾选警告后继续".into());
    }
    let p =
        mosaic_core::protect_grid(&base, password, mode, selection).map_err(|e| e.to_string())?;
    let is_block = mode == Mode::Entropy && !selection.is_empty();
    Ok(Document {
        bytes: p.bitmap,
        sidecar: p.sidecar,
        name: format!(
            "{}-edited.bmp",
            d.name.trim_end_matches(".bmp").trim_end_matches("-edited")
        ),
        prepared: None,
        block_origin: if is_block { Some(base) } else { None },
        block_selection: if is_block {
            Some(selection.clone())
        } else {
            None
        },
    })
}
#[derive(Serialize)]
struct Suggestions {
    selection: GridSelection,
    uncertain_blocks: usize,
    source: &'static str,
}
#[tauri::command]
async fn suggest_blocks(app: tauri::AppHandle, block_size: u32) -> Result<Suggestions, String> {
    blocking(move || {
        let state: State<Session> = app.state();
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let d = guard.as_ref().ok_or("请先打开 BMP")?;
        if let Some(known) = &d.block_selection {
            if known.block_size != block_size {
                return Err(format!(
                    "本次会话的加密区块为 {} px，请选择相同尺寸",
                    known.block_size
                ));
            }
            return Ok(Suggestions {
                selection: known.clone(),
                uncertain_blocks: 0,
                source: "session",
            });
        }
        let report = mosaic_core::detect_blocks(&d.bytes, block_size).map_err(|e| e.to_string())?;
        Ok(Suggestions {
            selection: report.selection,
            uncertain_blocks: report.uncertain_blocks,
            source: "heuristic",
        })
    })
    .await
}
#[tauri::command]
async fn preview_blocks(
    app: tauri::AppHandle,
    password: String,
    selection: GridSelection,
    existing_selection: Option<GridSelection>,
) -> Result<mosaic_core::EncryptionPreview, String> {
    let password = Zeroizing::new(password);
    blocking(move || {
        let state: State<Session> = app.state();
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        let d = guard.as_mut().ok_or("请先打开 BMP")?;
        prepare_document(d, &password, &selection, existing_selection.as_ref())
    })
    .await
}
fn prepare_document(
    d: &mut Document,
    password: &str,
    selection: &GridSelection,
    existing: Option<&GridSelection>,
) -> Result<mosaic_core::EncryptionPreview, String> {
    d.prepared = None;
    let candidate = protect_document(d, password, Mode::Entropy, selection, false, existing)?;
    let report =
        mosaic_core::assess_encryption(&candidate.bytes, selection).map_err(|e| e.to_string())?;
    d.prepared = Some(Box::new(PreparedDocument {
        document: candidate,
        report: report.clone(),
    }));
    Ok(report)
}
fn finish_preview(d: &mut Document, proceed: bool) -> Result<Option<View>, String> {
    if !proceed {
        d.prepared = None;
        return Ok(None);
    }
    let candidate = d.prepared.as_ref().ok_or("预演已失效，请重新预演")?;
    let v = view(&candidate.document)?;
    let candidate = d.prepared.take().ok_or("预演已失效")?;
    *d = candidate.document;
    Ok(Some(v))
}
#[tauri::command]
async fn commit_preview(app: tauri::AppHandle) -> Result<Option<View>, String> {
    blocking(move||{let state:State<Session>=app.state();let mut guard=state.0.lock().map_err(|e|e.to_string())?;let d=guard.as_mut().ok_or("请先打开 BMP")?;
        let report=&d.prepared.as_ref().ok_or("预演已失效，请重新预演")?.report;
        let proceed=if report.warning {
            let score=report.entropy_auc.map(|v|format!("{:.1}%",v*100.0)).unwrap_or_else(||"没有两类样本，无法比较".into());
            app.dialog().message(format!("预演显示自动识别可能不准确。\n熵区分度（AUC）：{score}\n预计误选 {} 块、漏选 {} 块，样本不足边缘格 {} 块。\n\n仍然继续加密吗？取消会保留当前图片。",report.false_positive_blocks,report.missed_blocks,report.uncertain_blocks))
                .title("区块加密预演警告").kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom("继续加密".into(),"取消".into())).blocking_show()
        }else{true};
        finish_preview(d,proceed)
    }).await
}
fn write_new(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("无法保存（请使用新文件名）：{e}"))?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(e.to_string());
    }
    Ok(())
}
#[tauri::command]
async fn save_document(app: tauri::AppHandle) -> Result<Option<String>, String> {
    blocking(move || {
        let state: State<Session> = app.state();
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let d = guard.as_ref().ok_or("请先打开 BMP")?;
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name(&d.name)
            .add_filter("BMP 图片", &["bmp"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = file.into_path().map_err(|e| e.to_string())?;
        if path.extension().is_none() {
            path.set_extension("bmp");
        }
        let side_path = PathBuf::from(format!("{}.lmm", path.display()));
        if side_path.exists() {
            return Err("目标位置已有同名还原信息，请使用新文件名".into());
        }
        write_new(&path, &d.bytes)?;
        if let Some(s) = &d.sidecar {
            if let Err(e) = write_new(&side_path, s) {
                let _ = fs::remove_file(&path);
                return Err(e);
            }
        }
        Ok(Some(if d.sidecar.is_some() {
            format!("{}（及同名 .lmm 还原文件）", path.display())
        } else {
            path.display().to_string()
        }))
    })
    .await
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Session::default())
        .invoke_handler(tauri::generate_handler![
            open_document,
            attach_sidecar,
            transform,
            suggest_blocks,
            preview_blocks,
            commit_preview,
            save_document
        ])
        .run(tauri::generate_context!())
        .expect("无法启动桌面应用");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document() -> Document {
        let width = 64u32;
        let height = 64u32;
        let len = 54 + width as usize * height as usize * 3;
        let mut bytes = vec![90; len];
        bytes[..54].fill(0);
        bytes[..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(len as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&width.to_le_bytes());
        bytes[22..26].copy_from_slice(&height.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
        Document {
            bytes,
            sidecar: None,
            name: "preview.bmp".into(),
            block_origin: None,
            block_selection: None,
            prepared: None,
        }
    }
    #[test]
    fn warning_preview_can_be_cancelled_or_explicitly_committed() {
        let mut d = document();
        let before = d.bytes.clone();
        let selection = GridSelection::from_regions(
            64,
            64,
            8,
            &[mosaic_core::Rect {
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            }],
        )
        .unwrap();
        let report = prepare_document(&mut d, "preview-test", &selection, None).unwrap();
        assert!(report.warning);
        assert_eq!(d.bytes, before);
        assert!(finish_preview(&mut d, false).unwrap().is_none());
        assert_eq!(d.bytes, before);
        assert!(d.prepared.is_none());
        prepare_document(&mut d, "preview-test", &selection, None).unwrap();
        let candidate = d.prepared.as_ref().unwrap().document.bytes.clone();
        assert!(finish_preview(&mut d, true).unwrap().is_some());
        assert_eq!(d.bytes, candidate);
        assert_ne!(d.bytes, before);
        assert!(finish_preview(&mut d, true).is_err());
        assert_eq!(
            mosaic_core::transform_grid(&d.bytes, "preview-test", &selection).unwrap(),
            before
        );
    }
}
