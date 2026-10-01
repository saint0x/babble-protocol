use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let Some(kind) = args.next() else {
        return Err("usage: babel-schema-export <bundle|fixtures> [output-path]".into());
    };
    let output = match kind.as_str() {
        "bundle" => babel_schema::protocol_schema_bundle_json()?,
        "fixtures" => babel_schema::protocol_fixtures_json()?,
        other => return Err(format!("unsupported export kind: {other}").into()),
    };

    if let Some(path) = args.next() {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, output)?;
    } else {
        print!("{output}");
    }
    Ok(())
}
