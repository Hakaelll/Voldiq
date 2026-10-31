use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
pub(crate) struct Report {
    probe: &'static str,
    development: bool,
    assets: Vec<Asset>,
}

#[derive(Serialize)]
struct Asset {
    path: String,
    bytes: usize,
    sha256: String,
}

pub(crate) fn verify(context: &tauri::Context<tauri::Wry>) -> Result<Report, String> {
    if tauri::is_dev() {
        return Err(
            "This executable uses the development server instead of embedded UI assets.".into(),
        );
    }
    let assets: Vec<_> = context
        .assets()
        .iter()
        .map(|(path, _)| {
            let bytes = context
                .assets()
                .get(&path.as_ref().into())
                .ok_or_else(|| format!("Embedded asset cannot be decoded: {path}"))?;
            Ok(Asset {
                path: path.trim_start_matches('/').to_owned(),
                bytes: bytes.len(),
                sha256: format!("{:X}", Sha256::digest(&bytes)),
            })
        })
        .collect::<Result<_, String>>()?;
    let index = context
        .assets()
        .get(&"index.html".into())
        .ok_or("Embedded index.html is missing.")?;
    let html = std::str::from_utf8(&index).map_err(|error| error.to_string())?;
    for extension in [".js", ".css"] {
        if !assets.iter().any(|asset| {
            asset.path.starts_with("assets/")
                && asset.path.ends_with(extension)
                && asset.bytes > 0
                && html.contains(&asset.path)
        }) {
            return Err(format!(
                "Embedded index.html has no matching {extension} payload."
            ));
        }
    }
    Ok(Report {
        probe: "VOLDIQ_EMBEDDED_UI_V1",
        development: false,
        assets,
    })
}
