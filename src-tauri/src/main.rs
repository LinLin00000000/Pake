#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod util;

use app::{invoke, menu, window};
use invoke::{download_file, download_file_by_binary};
use menu::{get_menu, menu_event_handle};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{env, path::PathBuf, sync::mpsc::channel};
use tauri::Manager;
use tauri_plugin_window_state::Builder as windowStatePlugin;
use tokio::fs;
use tokio::time::{sleep, Duration};
use util::{get_data_dir, get_pake_config};
use window::get_window;

#[tokio::main]
async fn main() {
    let (pake_config, tauri_config) = get_pake_config();
    let show_menu = pake_config.show_menu();
    let menu = get_menu();
    let data_dir = get_data_dir(tauri_config);

    let mut tauri_app = tauri::Builder::default();

    if show_menu {
        tauri_app = tauri_app.menu(menu).on_menu_event(menu_event_handle);
    }

    #[cfg(not(target_os = "macos"))]
    {
        use menu::{get_system_tray, system_tray_handle};

        let show_system_tray = pake_config.show_system_tray();
        let system_tray = get_system_tray(show_menu);

        if show_system_tray {
            tauri_app = tauri_app
                .system_tray(system_tray)
                .on_system_tray_event(system_tray_handle);
        }
    }

    // 设置要监听的文件夹
    let home_dir = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .expect("无法找到 home 目录");
    let watch_folder = PathBuf::from(home_dir)
        .join("Documents")
        .join("Escape from Tarkov")
        .join("Screenshots");

    // 创建一个通道来接收事件
    let (tx, rx) = channel();

    // 创建并启动监听器
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            if let Err(e) = tx.send(res) {
                println!("发送事件时出错: {:?}", e);
            }
        },
        notify::Config::default(),
    )
    .unwrap();
    watcher
        .watch(&watch_folder, RecursiveMode::Recursive)
        .unwrap();

    tauri_app
        .plugin(windowStatePlugin::default().build())
        .invoke_handler(tauri::generate_handler![
            download_file,
            download_file_by_binary
        ])
        .setup(|app| {
            let app_handle = app.handle();
            // 在单独的线程中处理文件系统事件
            tokio::spawn(async move {
                for res in rx {
                    match res {
                        Ok(notify::Event {
                            kind: notify::EventKind::Create(_),
                            paths,
                            ..
                        }) => {
                            if let Some(path) = paths
                                .get(0)
                                .and_then(|p| p.file_stem())
                                .and_then(|f| f.to_str())
                            {
                                let filename = path.to_string();
                                println!("检测到新文件: {}", filename); // 输出新文件检测信息
                                app_handle.emit_all("gps", &filename).unwrap(); // 更改事件名为 "gps"

                                let path_clone = paths[0].clone();
                                println!("准备删除文件: {:?}", path_clone);
                                tokio::spawn(async move {
                                    sleep(Duration::from_secs(3)).await;
                                    match fs::remove_file(&path_clone).await {
                                        Ok(_) => println!("文件 {:?} 已删除", path_clone),
                                        Err(e) => {
                                            println!("删除文件 {:?} 时出错: {}", path_clone, e)
                                        }
                                    }
                                });
                            }
                        }
                        Ok(_) => {
                            // 可以选择不执行任何操作或输出一些日志信息
                            println!("其他类型的文件事件被忽略");
                        }
                        Err(e) => println!("监听错误: {:?}", e),
                    }
                }
            });

            let _window = get_window(app, pake_config, data_dir);
            // Prevent initial shaking
            _window.show().unwrap();
            Ok(())
        })
        .on_window_event(|event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
                #[cfg(target_os = "macos")]
                {
                    event.window().minimize().unwrap();
                }

                #[cfg(not(target_os = "macos"))]
                event.window().close().unwrap();

                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
