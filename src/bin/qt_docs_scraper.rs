use std::{collections::HashMap, fs, path::PathBuf};

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;

#[derive(Serialize)]
struct QtComponentDocs {
    description: String,
    properties: HashMap<String, String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut docs: HashMap<String, QtComponentDocs> = HashMap::new();

    // this should maybe not be hardcoded
    // but qmake returns a bad path (/usr/share/doc/qt) so idk
    let qt_docs_path = "/usr/share/doc/qt6".to_string();
    let docs_root = PathBuf::from(qt_docs_path);
    if !docs_root.exists() {
        eprintln!("Documentation dir not found. Make sure qt6-doc is installed.");
        return Ok(());
    }

    let title_selector = Selector::parse("h1.title").unwrap();
    let descr_selector = Selector::parse("h1.title ~ p").unwrap();
    let prop_header_selector = Selector::parse("h3.qml-property").unwrap();
    let prop_name_selector = Selector::parse("span.name").unwrap();

    let mut files_to_visit = vec![docs_root];

    while let Some(dir) = files_to_visit.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files_to_visit.push(path);
            } else if let Some(filename) = path.file_name().and_then(|n| n.to_str())
                && filename.starts_with("qml-")
                && filename.ends_with(".html")
                && !filename.ends_with("-members.html")
            {
                let Ok(html) = fs::read_to_string(&path) else {
                    continue;
                };
                let document = Html::parse_document(&html);

                let component_name = document
                    .select(&title_selector)
                    .next()
                    .map(|h1| h1.text().collect::<Vec<_>>().join(""))
                    .map(|title| title.replace(" QML Type", "").trim().to_string());

                let description = document.select(&descr_selector).next().map(|p| {
                    p.text()
                        .collect::<Vec<_>>()
                        .join("")
                        .replace("More...", "")
                        .trim()
                        .to_string()
                });

                if let (Some(name), Some(desc)) = (component_name, description) {
                    if !name.is_empty() && !desc.is_empty() {
                        let mut properties = HashMap::new();

                        for prop_header in document.select(&prop_header_selector) {
                            let prop_name = prop_header
                                .select(&prop_name_selector)
                                .next()
                                .map(|n| n.text().collect::<String>().trim().to_string());

                            if let Some(prop_name) = prop_name {
                                let mut next_node = prop_header.next_sibling();
                                let mut prop_desc = String::new();

                                while let Some(node) = next_node {
                                    if let Some(element) = ElementRef::wrap(node) {
                                        if element.value().name() == "p" {
                                            prop_desc = element
                                                .text()
                                                .collect::<Vec<_>>()
                                                .join("")
                                                .trim()
                                                .to_string();
                                        }
                                        break;
                                    }
                                    next_node = node.next_sibling();
                                }

                                if !prop_desc.is_empty() {
                                    properties.insert(prop_name, prop_desc);
                                }
                            }
                        }

                        docs.insert(
                            name,
                            QtComponentDocs {
                                description: desc,
                                properties,
                            },
                        );
                    }
                }
            }
        }
    }

    fs::create_dir_all("data")?;
    let json = serde_json::to_string_pretty(&docs)?;
    fs::write("data/qt_docs.json", json)?;

    Ok(())
}
