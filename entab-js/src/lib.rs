mod utils;

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use entab_base::buffer::ReadBuffer;
use entab_base::compression::decompress;
use entab_base::readers::{get_reader, RecordReader};
use entab_base::record::Value;
use entab_base::utils::error::EtError;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[derive(Serialize)]
pub struct NextRecord<'v> {
    value: Option<BTreeMap<&'v str, Value>>,
    done: bool,
}

#[wasm_bindgen]
pub struct Reader {
    parser: String,
    headers: Vec<String>,
    reader: Box<dyn RecordReader>,
}

fn to_js(err: EtError) -> JsValue {
    err.to_string().into()
}

#[wasm_bindgen]
impl Reader {
    #[wasm_bindgen(constructor)]
    pub fn new(data: Box<[u8]>, parser: Option<String>) -> Result<Reader, JsValue> {
        utils::set_panic_hook();

        let stream: Box<dyn Read> = Box::new(Cursor::new(data));

        let (reader, filetype, _) = decompress(stream).map_err(to_js)?;
        let buffer = ReadBuffer::new(reader).map_err(to_js)?;

        let parser_name = parser.unwrap_or_else(|| filetype.to_parser_name().to_string());
        let reader = get_reader(&parser_name, buffer).map_err(to_js)?;
        let headers = reader.headers();
        Ok(Reader {
            parser: parser_name.to_string(),
            headers,
            reader,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn parser(&self) -> String {
        self.parser.clone()
    }

    // FIXME: it'd be nice to implement iterable
    // #[wasm_bindgen(js_name = "@@iterable")]
    // pub fn iterable(&self) -> JsValue {
    //     self
    // }

    #[wasm_bindgen]
    pub fn next(&mut self) -> Result<JsValue, JsValue> {
        if let Some(value) = self.reader.next_record().map_err(to_js)? {
            let obj: BTreeMap<&str, Value> = self
                .headers
                .iter()
                .map(|i| i.as_ref())
                .zip(value.into_iter())
                .collect();
            JsValue::from_serde(&NextRecord {
                value: Some(obj),
                done: false,
            })
            .map_err(|_| JsValue::from_str("Error translating record"))
        } else {
            JsValue::from_serde(&NextRecord {
                value: None,
                done: false,
            })
            .map_err(|_| JsValue::from_str("Error translating record"))
        }
    }
}
