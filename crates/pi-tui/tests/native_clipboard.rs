//! The arboard-backed native clipboard against a real X server. Linux only; starts its own `Xvfb`
//! (skipped when it is not installed).

#![cfg(target_os = "linux")]

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use pi_tui::native_platform::{ClipboardRead, get_native_clipboard};

struct Xvfb(Child);

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_xvfb() -> Option<(Xvfb, String)> {
    let display = format!(":{}", 90 + std::process::id() % 400);
    let child = Command::new("Xvfb")
        .args([display.as_str(), "-screen", "0", "64x64x24", "-nolisten", "tcp"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let socket = format!("/tmp/.X11-unix/X{}", &display[1..]);
    for _ in 0..100 {
        if std::path::Path::new(&socket).exists() {
            return Some((Xvfb(child), display));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[tokio::test]
async fn reads_text_image_and_files_from_x11() {
    let Some((_server, display)) = start_xvfb() else {
        eprintln!("skipping: Xvfb is not available");
        return;
    };
    // SAFETY: this test binary has a single test; nothing else reads the environment concurrently.
    unsafe { std::env::set_var("DISPLAY", &display) };
    let native = get_native_clipboard().expect("an X11 display enables the native helper");
    // Like the TS X11 helper, Linux writes go through command-line tools.
    assert!(!native.can_set_text());
    assert!(native.set_text("x").is_none());

    // Another clipboard owner (arboard keeps ownership while the handle lives).
    let mut owner = arboard::Clipboard::new().unwrap();
    assert_eq!(native.get_text().await, Ok(ClipboardRead::Empty));

    owner.set_text("héllo from x11").unwrap();
    assert_eq!(native.get_text().await, Ok(ClipboardRead::Data("héllo from x11".to_string())));

    owner
        .set_image(arboard::ImageData { width: 1, height: 1, bytes: vec![1, 2, 3, 255].into() })
        .unwrap();
    match native.get_image().await {
        Ok(ClipboardRead::Data(png)) => assert!(png.starts_with(b"\x89PNG"), "not a PNG"),
        other => panic!("expected an image, got {other:?}"),
    }

    let dir = std::env::temp_dir().join(format!("pi-tui-clip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a b.txt");
    std::fs::write(&file, "x").unwrap();
    let file = file.canonicalize().unwrap();
    owner.set().file_list(&[&file]).unwrap();
    let files = native.get_file_paths().expect("file paths are supported").await;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(files, Ok(ClipboardRead::Data(vec![file.display().to_string()])));
}
