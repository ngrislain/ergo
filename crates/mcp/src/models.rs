//! Read-only public/local model discovery. No scene edits, physics or rendering.
use anyhow::{Context, Result, ensure};
use ergo_world::ldraw;
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

const ORIGIN: &str = "https://library.ldraw.org";
const MAX_BYTES: usize = 8 * 1024 * 1024;
fn selector(value: &str) -> Selector {
    Selector::parse(value).unwrap()
}
fn text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .cookie_store(true)
        .timeout(Duration::from_secs(20))
        .user_agent("Ergo MCP model-reference library (https://ergosandbox.com)")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 3
                && attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some("library.ldraw.org" | "rebrickable.com" | "www.bricklink.com")
                )
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()?)
}
async fn fetch(
    http: &reqwest::Client,
    url: reqwest::Url,
    api_key: Option<&str>,
) -> Result<Vec<u8>> {
    let mut request = http.get(url);
    if let Some(key) = api_key {
        request = request.header("Authorization", format!("key {key}"));
    }
    let response = request.send().await?.error_for_status()?;
    read_response(response).await
}
async fn read_response(mut response: reqwest::Response) -> Result<Vec<u8>> {
    ensure!(
        response
            .content_length()
            .is_none_or(|bytes| bytes <= MAX_BYTES as u64),
        "Model reference exceeds 8 MiB"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MAX_BYTES,
            "Model reference exceeds 8 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn ldraw_search(http: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Value>> {
    // The public OMR table persists search in its guest session; a query-string
    // parameter alone returns the unfiltered index. Use its actual read-only
    // Livewire search protocol, with a fresh provider-owned cookie/CSRF session.
    let html = String::from_utf8(
        fetch(
            http,
            reqwest::Url::parse(&format!("{ORIGIN}/omr/sets"))?,
            None,
        )
        .await?,
    )?;
    let document = Html::parse_document(&html);
    let update = document
        .select(&selector("script[data-update-uri]"))
        .next()
        .and_then(|script| script.value().attr("data-update-uri"))
        .context("OMR search endpoint missing")?;
    let update = reqwest::Url::parse(ORIGIN)?.join(update)?;
    ensure!(
        update.scheme() == "https"
            && update.host_str() == Some("library.ldraw.org")
            && update.path().starts_with("/livewire")
            && update.path().ends_with("/update"),
        "Unexpected OMR search endpoint"
    );
    let csrf = document
        .select(&selector("meta[name='csrf-token']"))
        .next()
        .and_then(|meta| meta.value().attr("content"))
        .context("OMR search token missing")?
        .to_owned();
    let snapshot = document
        .select(&selector("[wire\\:snapshot]"))
        .filter_map(|element| element.value().attr("wire:snapshot"))
        .find(|snapshot| {
            serde_json::from_str::<Value>(snapshot)
                .ok()
                .is_some_and(|value| {
                    value["data"].get("tableSearch").is_some()
                        && value["data"].get("isTableLoaded").is_some()
                })
        })
        .context("OMR search component unavailable")?
        .to_owned();
    let response=http.post(update).header("X-Livewire","true")
        .json(&json!({"_token":csrf,"components":[{"snapshot":snapshot,"updates":{"tableSearch":query},"calls":[]}]})).send().await?.error_for_status()?;
    let result: Value = serde_json::from_slice(&read_response(response).await?)?;
    let html = result["components"][0]["effects"]["html"]
        .as_str()
        .context("OMR search response omitted results")?;
    Ok(set_rows(html, limit))
}
fn set_rows(html: &str, limit: usize) -> Vec<Value> {
    let document = Html::parse_document(html);
    let mut result = Vec::new();
    for row in document.select(&selector("table tbody tr")) {
        let cells: Vec<_> = row.select(&selector("td")).collect();
        if cells.len() < 5 {
            continue;
        }
        let Some(url) = row
            .select(&selector("a[href]"))
            .filter_map(|link| link.value().attr("href"))
            .find(|href| href.contains("/omr/sets/"))
        else {
            continue;
        };
        let Some(id) = url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|id| id.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        result.push(json!({"model_id":format!("ldraw:{id}"),"source":"ldraw","kind":"official_set","set_number":text(cells[1]),"name":text(cells[2]),"theme":text(cells[3]),"year":text(cells[4]),"source_url":url,"thumbnail_url":row.select(&selector("img")).next().and_then(|image|image.value().attr("src")),"retrieval":"Call get_lego_models with this model_id; inspect variants/submodels before reuse"}));
        if result.len() >= limit {
            break;
        }
    }
    result
}
fn downloads(html: &str) -> Vec<(String, String)> {
    let document = Html::parse_document(html);
    let mut results = BTreeMap::new();
    for link in document.select(&selector("a[href]")) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let Ok(url) = reqwest::Url::parse(href) else {
            continue;
        };
        if url.scheme() != "https"
            || url.host_str() != Some("library.ldraw.org")
            || !url.path().starts_with("/media/omr_models/")
            || ![".mpd", ".ldr"]
                .iter()
                .any(|suffix| url.path().to_lowercase().ends_with(suffix))
        {
            continue;
        }
        let Some(id) = url
            .path_segments()
            .and_then(|mut parts| parts.nth(2))
            .filter(|id| id.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        results.insert(id.to_owned(), url.to_string());
    }
    results.into_iter().collect()
}
fn selected_document(
    raw: &str,
    submodel: Option<&str>,
) -> Result<(String, Vec<Value>, Vec<String>)> {
    let mut document = ldraw::Document::parse(raw, "reference.mpd")?;
    let names: Vec<_> = document
        .files
        .keys()
        .filter(|name| name.ends_with(".ldr") || name.ends_with(".mpd"))
        .cloned()
        .collect();
    if let Some(submodel) = submodel {
        let root = ldraw::normalize_name(submodel)?;
        ensure!(
            document.files.contains_key(&root),
            "Requested submodel is not present"
        );
        document.root = root;
    }
    let mut inventory = BTreeMap::<String, usize>::new();
    for instance in document.instances()? {
        *inventory.entry(instance.name).or_default() += 1;
    }
    let output = if submodel.is_some() {
        let mut output = format!(
            "0 FILE {}\n{}\n",
            document.root, document.files[&document.root]
        );
        let mut reachable = BTreeSet::from([document.root.clone()]);
        let mut queue = vec![document.root.clone()];
        while let Some(name) = queue.pop() {
            for line in document.files[&name]
                .lines()
                .filter(|line| line.split_whitespace().next() == Some("1"))
            {
                let file = line
                    .split_whitespace()
                    .skip(14)
                    .collect::<Vec<_>>()
                    .join(" ");
                let file = ldraw::normalize_name(&file)?;
                if document.files.contains_key(&file) && reachable.insert(file.clone()) {
                    queue.push(file);
                }
            }
        }
        for (name, contents) in &document.files {
            if *name != document.root && reachable.contains(name) {
                output.push_str(&format!("0 FILE {name}\n{contents}\n"));
            }
        }
        output
    } else {
        raw.to_owned()
    };
    Ok((
        output,
        inventory
            .into_iter()
            .map(|(part, count)| json!({"part":part,"count":count}))
            .collect(),
        names,
    ))
}
fn reference(raw: &str, source_url: &str, id: &str, submodel: Option<&str>) -> Result<Value> {
    let author = raw
        .lines()
        .find_map(|line| line.strip_prefix("0 Author: "))
        .unwrap_or("See source document");
    let license = raw
        .lines()
        .find_map(|line| line.strip_prefix("0 !LICENSE "))
        .unwrap_or("Unspecified; inspect source before reuse");
    let (document, inventory, submodels) = selected_document(raw, submodel)?;
    let digest = format!("{:x}", Sha256::digest(document.as_bytes()));
    let mut value = json!({"model_id":id,"source_url":source_url,"author":author,"license":license,"sha256":digest,"part_inventory":inventory,"submodels":submodels,
        "reuse":"Use document with place_submodel(document=...), or use its part choices and construction pattern as a reference. Retain source/author/license attribution.",
        "physical_readiness":"Reference only: resolve genuine catalogue geometry/masses; placement and authoritative publication still check proper metric isometries, support and permissions. Catalogue inventories are not model poses."});
    if document.len() <= 128 * 1024 {
        value["document"] = json!(document);
    } else {
        let directory = std::env::var_os("ERGO_MODEL_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".cache/ergo/model-references"))
            })
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("ergo-model-references-{}", uuid::Uuid::new_v4()))
            });
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = directory.join(format!("{digest}.mpd"));
        if !path.exists() {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&path)?.write_all(document.as_bytes())?;
        }
        value["document_cache_path"] = json!(path);
        value["document_note"] = json!(
            "Large reference stored locally; choose a smaller submodel or inspect the cached source. No geometry was placed."
        );
    }
    Ok(value)
}
#[derive(Deserialize)]
struct Custom {
    id: String,
    name: String,
    path: String,
    author: String,
    license: String,
    #[serde(default)]
    tags: Vec<String>,
}
fn custom(
    root: &Path,
    query: &str,
    id: Option<&str>,
    submodel: Option<&str>,
    limit: usize,
) -> Result<Value> {
    let root = root.canonicalize()?;
    let index: Vec<Custom> = serde_json::from_slice(&std::fs::read(root.join("index.json"))?)?;
    if let Some(id) = id {
        let item = index
            .into_iter()
            .find(|item| item.id == id)
            .context("Custom model ID not found")?;
        let path = root.join(&item.path).canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "Custom model must stay inside its configured library"
        );
        ensure!(
            std::fs::metadata(&path)?.len() <= MAX_BYTES as u64,
            "Custom model exceeds 8 MiB"
        );
        let mut value = reference(
            &std::fs::read_to_string(&path)?,
            &format!("custom:{}", item.id),
            &format!("custom:{}", item.id),
            submodel,
        )?;
        value["author"] = json!(item.author);
        value["license"] = json!(item.license);
        value["name"] = json!(item.name);
        return Ok(value);
    }
    let tokens: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut results:Vec<_>=index.into_iter().filter_map(|item| {
        let haystack=format!("{} {}",item.name,item.tags.join(" ")).to_lowercase();
        let score=tokens.iter().filter(|token|haystack.contains(token.as_str())).count();
        (score>0).then(||(score,json!({"model_id":format!("custom:{}",item.id),"source":"custom","kind":"custom_model","name":item.name,"author":item.author,"license":item.license,"tags":item.tags})))
    }).collect();
    results.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    Ok(json!(
        results
            .into_iter()
            .take(limit)
            .map(|(_, value)| value)
            .collect::<Vec<_>>()
    ))
}

pub async fn call(arguments: Value) -> Result<Value> {
    let query = arguments["query"].as_str().unwrap_or("").trim();
    let model_id = arguments["model_id"].as_str();
    let source = arguments["source"].as_str().unwrap_or("all");
    let limit = arguments["limit"].as_u64().unwrap_or(5) as usize;
    let submodel = arguments["submodel"].as_str();
    ensure!(
        (1..=10).contains(&limit) && query.len() <= 200,
        "Use a query up to 200 bytes and limit 1..10"
    );
    ensure!(
        model_id.is_some() || !query.is_empty(),
        "Supply query or model_id"
    );
    let http = client()?;
    if let Some(id) = model_id {
        if let Some(id) = id.strip_prefix("custom:") {
            let root = std::env::var_os("ERGO_MODEL_LIBRARY_DIR")
                .context("Configure ERGO_MODEL_LIBRARY_DIR for custom models")?;
            return custom(Path::new(&root), query, Some(id), submodel, limit);
        }
        if let Some(id) = id.strip_prefix("rebrickable:") {
            ensure!(
                id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                "Invalid set number"
            );
            let key = std::env::var("ERGO_REBRICKABLE_API_KEY")
                .context("Rebrickable API access requires ERGO_REBRICKABLE_API_KEY")?;
            let url =
                reqwest::Url::parse(&format!("https://rebrickable.com/api/v3/lego/sets/{id}/"))?;
            let value: Value = serde_json::from_slice(&fetch(&http, url, Some(&key)).await?)?;
            return Ok(
                json!({"model_id":format!("rebrickable:{id}"),"kind":"catalogue_reference","catalogue":value,"document_available":false,"note":"Catalogue metadata/inventory is not a spatial LDraw model; use the source instructions or find an equivalent LDraw/custom model."}),
            );
        }
        let parts: Vec<_> = id.split(':').collect();
        ensure!(
            parts.len() >= 2
                && parts.len() <= 3
                && parts[0] == "ldraw"
                && parts[1..].iter().all(|part| !part.is_empty()
                    && part.len() <= 12
                    && part.bytes().all(|b| b.is_ascii_digit())),
            "Use ldraw:SET_ID[:MODEL_ID], rebrickable:SET_NUMBER or custom:ID"
        );
        let page_url = format!("{ORIGIN}/omr/sets/{}", parts[1]);
        let page = String::from_utf8(fetch(&http, reqwest::Url::parse(&page_url)?, None).await?)?;
        let variants = downloads(&page);
        if parts.len() == 2 && variants.len() > 1 {
            return Ok(
                json!({"source_url":page_url,"variants":variants.iter().map(|(model,url)|json!({"model_id":format!("ldraw:{}:{model}",parts[1]),"document_url":url})).collect::<Vec<_>>(),"note":"Choose the relevant official/alternate model before retrieving its document"}),
            );
        }
        let (model, url) = variants
            .iter()
            .find(|(id, _)| parts.len() == 2 || *id == parts[2])
            .context("No matching downloadable LDraw model")?;
        let raw = String::from_utf8(fetch(&http, reqwest::Url::parse(url)?, None).await?)?;
        let mut value = reference(&raw, url, &format!("ldraw:{}:{model}", parts[1]), submodel)?;
        value["catalogue_url"] = json!(page_url);
        return Ok(value);
    }
    let mut models = Vec::new();
    let mut providers = Vec::new();
    if source == "all" || source == "ldraw" {
        match ldraw_search(&http, query, limit).await {
            Ok(rows) => {
                models.extend(rows);
                providers.push(json!({"source":"ldraw","status":"searched"}));
            }
            Err(error) => providers
                .push(json!({"source":"ldraw","status":"unavailable","error":error.to_string()})),
        }
    }
    if (source == "all" || source == "custom")
        && let Some(root) = std::env::var_os("ERGO_MODEL_LIBRARY_DIR")
    {
        models.extend(
            custom(Path::new(&root), query, None, None, limit)?
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        providers.push(json!({"source":"custom","status":"searched"}));
    }
    if source == "custom" && std::env::var_os("ERGO_MODEL_LIBRARY_DIR").is_none() {
        providers.push(json!({"source":"custom","status":"configuration_required","configuration":"ERGO_MODEL_LIBRARY_DIR/index.json"}));
    }
    if source == "all" || source == "rebrickable" {
        if let Ok(key) = std::env::var("ERGO_REBRICKABLE_API_KEY") {
            let mut url = reqwest::Url::parse("https://rebrickable.com/api/v3/lego/sets/")?;
            url.query_pairs_mut()
                .append_pair("search", query)
                .append_pair("page_size", &limit.to_string());
            match fetch(&http,url,Some(&key)).await.and_then(|bytes|Ok(serde_json::from_slice::<Value>(&bytes)?)) {
                Ok(value)=>{for item in value["results"].as_array().into_iter().flatten(){models.push(json!({"model_id":format!("rebrickable:{}",item["set_num"].as_str().unwrap_or_default()),"source":"rebrickable","kind":"catalogue_reference","name":item["name"],"set_number":item["set_num"],"thumbnail_url":item["set_img_url"],"source_url":item["set_url"],"part_count":item["num_parts"],"document_available":false}));}providers.push(json!({"source":"rebrickable","status":"searched"}));}
                Err(error)=>providers.push(json!({"source":"rebrickable","status":"unavailable","error":error.to_string()})),
            }
        } else {
            providers.push(json!({"source":"rebrickable","status":"api_key_required","configuration":"ERGO_REBRICKABLE_API_KEY"}));
        }
    }
    if source == "all" || source == "bricklink" {
        let mut url = reqwest::Url::parse("https://www.bricklink.com/catalogList.asp")?;
        url.query_pairs_mut()
            .append_pair("catType", "S")
            .append_pair("q", query);
        providers.push(json!({"source":"bricklink","status":"catalogue_reference_link","search_url":url.as_str(),"note":"BrickLink catalogue references do not supply reusable model poses; copyrighted/paid instructions are not downloaded."}));
    }
    models.truncate(limit);
    Ok(
        json!({"query":query,"models":models,"providers":providers,"guidance":"Retrieve and inspect the closest relevant official/custom model before designing. Reuse its document or adapt demonstrated part choices, shape and techniques; preserve attribution and user-requested scale. If no suitable match exists, report the searches and build an original model using sourced parts."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_public_omr_search_and_downloads_without_accepting_foreign_hosts() {
        let rows = set_rows(
            r#"<table><tbody><tr><td><a href="https://library.ldraw.org/omr/sets/610"><img src="car.png"></a></td><td>6505-1</td><td>Fire Chief's Car</td><td>Town</td><td>1988</td></tr></tbody></table>"#,
            5,
        );
        assert_eq!(rows[0]["model_id"], "ldraw:610");
        let links = downloads(
            r#"<a href="https://library.ldraw.org/media/omr_models/671/6505-1.mpd?v=1">Download</a><a href="https://example.test/media/omr_models/3/evil.mpd">No</a>"#,
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "671");
    }
    #[test]
    fn references_preserve_attribution_and_select_reusable_submodels() {
        let raw = "0 FILE main.ldr\n0 Author: Example Author\n0 !LICENSE CC BY 4.0\n1 4 0 0 0 1 0 0 0 1 0 0 0 1 car.ldr\n0 FILE car.ldr\n1 4 0 0 0 1 0 0 0 1 0 0 0 1 3001.dat\n";
        let value = reference(
            raw,
            "https://library.ldraw.org/example",
            "ldraw:1",
            Some("car.ldr"),
        )
        .unwrap();
        assert_eq!(value["author"], "Example Author");
        assert_eq!(value["part_inventory"][0]["part"], "3001.dat");
        assert!(
            value["document"]
                .as_str()
                .unwrap()
                .starts_with("0 FILE car.ldr")
        );
        assert!(
            !value["document"]
                .as_str()
                .unwrap()
                .contains("0 FILE main.ldr")
        );
        assert!(selected_document(raw, Some("../secret")).is_err());
    }
}
