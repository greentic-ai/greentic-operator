use std::fs::File;
use std::io::Read;

use serde_cbor::Value;

fn main() {
    let path = std::env::args().nth(1).expect("path");
    let file = File::open(&path).expect("open");
    let mut archive = zip::ZipArchive::new(file).expect("zip");
    let mut file = archive.by_name("manifest.cbor").expect("manifest");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).expect("read");
    let value: Value = serde_cbor::from_slice(&bytes).expect("cbor");
    let Value::Map(map) = value else {
        println!("not map");
        return;
    };
    println!("top keys:");
    for entry in &map {
        println!("  {:?}", entry.0);
    }
    if let Some(Value::Map(symbols)) = map.get(&Value::Text("symbols".to_string())) {
        println!("symbols keys:");
        for entry in symbols.iter() {
            println!("  {:?}", entry.0);
        }
        if let Some(v) = symbols.get(&Value::Text("pack_ids".to_string())) {
            println!("symbols.pack_ids: {v:?}");
        }
    } else {
        println!("symbols missing or not map");
    }
    if let Some(v) = map.get(&Value::Text("pack_id".to_string())) {
        println!("pack_id value: {v:?}");
    }
    if let Some(Value::Array(flows)) = map.get(&Value::Text("flows".to_string())) {
        for (idx, flow) in flows.iter().enumerate() {
            if let Value::Map(flow) = flow {
                if let Some(id) = flow.get(&Value::Text("id".to_string())) {
                    println!("flows[{idx}].id: {id:?}");
                }
                if let Some(entrypoints) = flow.get(&Value::Text("entrypoints".to_string())) {
                    println!("flows[{idx}].entrypoints: {entrypoints:?}");
                }
            }
        }
    }
}
