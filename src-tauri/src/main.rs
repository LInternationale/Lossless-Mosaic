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
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

const LIMIT: u64 = 64 * 1024 * 1024;
struct Document {
    bytes: Vec<u8>,
    sidecar: Option<Vec<u8>>,
    name: String,
    block_origin: Option<Vec<u8>>,
    block_selection: Option<GridSelection>,
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
                block_origin: None,
                block_selection: None,
            }
        } else {
            // Edit a target mask against the recovered source, so overlapping strokes never double-XOR.
            let base = if info.mode.is_some() {
                mosaic_core::restore(&d.bytes, d.sidecar.as_deref(), &password)
                    .map_err(|e| e.to_string())?
            } else {
                if let Some(origin) = &d.block_origin {
                    origin.clone()
                } else if let Some(existing) = &existing_selection {
                    mosaic_core::transform_grid(&d.bytes, &password, existing)
                        .map_err(|e| e.to_string())?
                } else {
                    d.bytes.clone()
                }
            };
            let source = Bitmap::parse(base.clone()).map_err(|e| e.to_string())?;
            if mode == Mode::Alpha && source.bpp == 32 && !allow_alpha_loss {
                return Err("α 模式会覆盖原有透明度，请勾选警告后继续".into());
            }
            let p = mosaic_core::protect_grid(&base, &password, mode, &selection)
                .map_err(|e| e.to_string())?;
            let is_block = mode == Mode::Entropy && !selection.is_empty();
            Document {
                bytes: p.bitmap,
                sidecar: p.sidecar,
                name: format!(
                    "{}-edited.bmp",
                    d.name.trim_end_matches(".bmp").trim_end_matches("-edited")
                ),
                block_origin: if is_block { Some(base) } else { None },
                block_selection: if is_block { Some(selection) } else { None },
            }
        };
        let v = view(&candidate)?;
        *d = candidate;
        Ok(v)
    })
    .await
}
#[tauri::command]
async fn suggest_blocks(app: tauri::AppHandle, block_size: u32) -> Result<GridSelection, String> {
    blocking(move || {
        let state: State<Session> = app.state();
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let d = guard.as_ref().ok_or("请先打开 BMP")?;
        mosaic_core::suggest_grid(&d.bytes, block_size).map_err(|e| e.to_string())
    })
    .await
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
            save_document
        ])
        .run(tauri::generate_context!())
        .expect("无法启动桌面应用");
}
