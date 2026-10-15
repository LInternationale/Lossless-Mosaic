#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use mosaic_core::{Bitmap, Mode, Rect};
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
}
#[derive(Default)]
struct Session(Mutex<Option<Document>>);
#[derive(Serialize)]
struct View {
    name: String,
    width: u32,
    height: u32,
    bpp: u16,
    mode: Option<Mode>,
    rgba: Vec<u8>,
    preview_width: u32,
    preview_height: u32,
    suggested_blocks: Vec<Rect>,
}
fn view(d: &Document) -> Result<View, String> {
    let i = mosaic_core::inspect(&d.bytes, d.sidecar.as_deref()).map_err(|e| e.to_string())?;
    let scale = (1600.0 / i.width.max(i.height) as f64).min(1.0);
    let w = (i.width as f64 * scale).round().max(1.0) as u32;
    let h = (i.height as f64 * scale).round().max(1.0) as u32;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let src = (((y as u64 * i.height as u64 / h as u64) * i.width as u64
                + x as u64 * i.width as u64 / w as u64)
                * 4) as usize;
            rgba.extend_from_slice(&i.rgba[src..src + 4]);
        }
    }
    Ok(View {
        name: d.name.clone(),
        width: i.width,
        height: i.height,
        bpp: i.bpp,
        mode: i.mode,
        rgba,
        preview_width: w,
        preview_height: h,
        suggested_blocks: i.suggested_blocks,
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
    regions: Vec<Rect>,
    decrypt: bool,
) -> Result<View, String> {
    let password = Zeroizing::new(password);
    blocking(move || {
        let state: State<Session> = app.state();
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        let d = guard.as_mut().ok_or("请先打开 BMP")?;
        let candidate = if decrypt {
            let bytes = if mode == Mode::Entropy {
                mosaic_core::transform_blocks(&d.bytes, &password, &regions)
            } else {
                mosaic_core::restore(&d.bytes, d.sidecar.as_deref(), &password)
            }
            .map_err(|e| e.to_string())?;
            Document {
                bytes,
                sidecar: None,
                name: format!("{}-restored.bmp", d.name.trim_end_matches(".bmp")),
            }
        } else {
            if mosaic_core::inspect(&d.bytes, d.sidecar.as_deref())
                .map_err(|e| e.to_string())?
                .mode
                .is_some()
            {
                return Err("请先还原后再重新打码".into());
            }
            let p = mosaic_core::protect(&d.bytes, &password, mode, &regions)
                .map_err(|e| e.to_string())?;
            Document {
                bytes: p.bitmap,
                sidecar: p.sidecar,
                name: format!("{}-protected.bmp", d.name.trim_end_matches(".bmp")),
            }
        };
        let v = view(&candidate)?;
        *d = candidate;
        Ok(v)
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
            save_document
        ])
        .run(tauri::generate_context!())
        .expect("无法启动桌面应用");
}
