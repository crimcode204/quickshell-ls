use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;

#[derive(Serialize)]
struct QtComponentDocs {
    description: String,
    properties: HashMap<String, String>,
}

fn find_doc_paths() -> Vec<PathBuf> {
    let paths: HashSet<PathBuf> = ["qtpaths6", "qtpaths", "qmake6", "qmake"]
        .into_iter()
        .filter_map(|tool| {
            let flag = if tool.starts_with("qmake") {
                "-query"
            } else {
                "--query"
            };
            std::process::Command::new(tool)
                .args([flag, "QT_INSTALL_DOCS"])
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .filter_map(|output| String::from_utf8(output.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.exists())
        .collect();

    paths.into_iter().collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files_to_visit = find_doc_paths();
    if files_to_visit.is_empty() {
        eprintln!("Documentation dir not found. Make sure qt-doc and/or qt6-doc are installed.");
        return Ok(());
    }

    let mut docs: HashMap<String, QtComponentDocs> = HashMap::new();
    let title_selector = Selector::parse("h1.title").unwrap();
    let descr_selector = Selector::parse("h1.title ~ p").unwrap();
    let prop_header_selector = Selector::parse("h3.qml-property").unwrap();
    let prop_name_selector = Selector::parse("span.name").unwrap();

    while let Some(dir) = files_to_visit.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                files_to_visit.push(path);
                continue;
            }

            let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            if !filename.starts_with("qml-")
                || !filename.ends_with(".html")
                || filename.ends_with("-members.html")
            {
                continue;
            }

            let Ok(html) = fs::read_to_string(&path) else {
                continue;
            };

            let document = Html::parse_document(&html);

            let Some(name) = document
                .select(&title_selector)
                .next()
                .map(|h1| {
                    h1.text()
                        .collect::<String>()
                        .replace(" QML Type", "")
                        .trim()
                        .to_string()
                })
                .filter(|name| !name.is_empty())
            else {
                continue;
            };

            let Some(description) = document
                .select(&descr_selector)
                .next()
                .map(|p| {
                    p.text()
                        .collect::<String>()
                        .replace("More...", "")
                        .trim()
                        .to_string()
                })
                .filter(|desc| !desc.is_empty())
            else {
                continue;
            };

            let properties: HashMap<String, String> = document
                .select(&prop_header_selector)
                .filter_map(|prop_header| {
                    let prop_name = prop_header
                        .select(&prop_name_selector)
                        .next()?
                        .text()
                        .collect::<String>()
                        .trim()
                        .to_string();

                    let prop_desc = std::iter::successors(prop_header.next_sibling(), |node| {
                        node.next_sibling()
                    })
                    .find_map(|node| ElementRef::wrap(node).filter(|e| e.value().name() == "p"))?
                    .text()
                    .collect::<String>()
                    .trim()
                    .to_string();

                    Some((prop_name, prop_desc)).filter(|(p, d)| !p.is_empty() && !d.is_empty())
                })
                .collect();

            docs.insert(
                name,
                QtComponentDocs {
                    description,
                    properties,
                },
            );
        }
    }

    fs::create_dir_all("data")?;
    let json = serde_json::to_string_pretty(&docs)?;
    fs::write("data/qt_docs.json", json)?;

    Ok(())
}
