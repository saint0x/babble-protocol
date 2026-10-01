use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

pub const HTML: &str = "<!doctype html><html><head><link rel=\"stylesheet\" href=\"./styles/site.css\"></head><body><main id=\"app\"></main><script type=\"module\" src=\"./scripts/main.js\"></script></body></html>\n";
pub const JS: &str = "import { text } from './nested/message.js'; document.getElementById('app').textContent = text;\n";
pub const MODULE: &str = "export const text = 'Portable Babel bundle';\n";
pub const CSS: &str = "@import './nested/colors.css'; body { color: var(--ink); }\n";
pub const COLORS: &str = ":root { --ink: #123456; }\n";

pub struct Fixture {
    pub root: PathBuf,
    pub manifest: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "babel-cli-bundle-{}-{nanos}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        for dir in ["dist/app/scripts/nested", "dist/app/styles/nested"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for (path, contents) in [
            ("dist/app/index.html", HTML),
            ("dist/app/scripts/main.js", JS),
            ("dist/app/scripts/nested/message.js", MODULE),
            ("dist/app/styles/site.css", CSS),
            ("dist/app/styles/nested/colors.css", COLORS),
        ] {
            fs::write(root.join(path), contents).unwrap();
        }
        let fixture = Self {
            manifest: root.join("manifest.json"),
            root,
        };
        fixture.write(&Self::input());
        fixture
    }

    pub fn input() -> Value {
        json!({
            "kind": "babel.text", "schema": "babel.schema.text.v1",
            "payload": {"text": "Bundle round trip", "metadata": {}},
            "surfaces": [{"role": "Feed", "target": "Web", "bundle": {
                "entry_path": "app/index.html",
                "files": [
                    {"path":"app/styles/site.css", "file":"dist/app/styles/site.css", "media_type":"text/css", "kind":"stylesheet"},
                    {"path":"app/scripts/main.js", "file":"dist/app/scripts/main.js", "media_type":"text/javascript", "kind":"script"},
                    {"path":"app/index.html", "file":"dist/app/index.html", "media_type":"text/html", "kind":"document"},
                    {"path":"app/styles/nested/colors.css", "file":"dist/app/styles/nested/colors.css", "media_type":"text/css", "kind":"stylesheet"},
                    {"path":"app/scripts/nested/message.js", "file":"dist/app/scripts/nested/message.js", "media_type":"text/javascript", "kind":"script"}
                ]
            }}]
        })
    }

    pub fn write(&self, value: &Value) {
        fs::write(&self.manifest, serde_json::to_vec(value).unwrap()).unwrap();
    }

    pub fn identity(&self) -> (PathBuf, PathBuf) {
        let identity = self.root.join("identity.json");
        let key = self.root.join("key.json");
        ok([
            "identity",
            "new",
            str_path(&identity),
            str_path(&key),
            "Application",
            "bundle-author",
        ]);
        (identity, key)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn str_path(path: &Path) -> &str {
    path.to_str().unwrap()
}

pub fn command<const N: usize>(args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_babel"))
        .args(args)
        .output()
        .unwrap()
}

pub fn ok<const N: usize>(args: [&str; N]) -> Value {
    let output = command(args);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

pub fn fails<const N: usize>(args: [&str; N], message: &str) {
    let output = command(args);
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stdout.is_empty(),
        "failure must not emit a verification report"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(message),
        "expected {message:?}, got {stderr}"
    );
}
