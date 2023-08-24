use crate::utils::console_log;
use anyhow::anyhow;
use serde::Deserialize;
use serde_json::json;
use subxt::{
    ext::codec::{Compact, Encode},
    tx::SubmittableExtrinsic,
    utils::Era,
    OnlineClient, PolkadotConfig,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = JSON, js_name = parse)]
    pub fn json_parse(string: String) -> JsValue;
}

fn to_hex(bytes: impl AsRef<[u8]>) -> String {
    format!("0x{}", hex::encode(bytes.as_ref()))
}

fn encode_then_hex<E: Encode>(input: &E) -> String {
    format!("0x{}", hex::encode(input.encode()))
}

pub async fn generate_payload(
    api: &OnlineClient<PolkadotConfig>,
    account_address: String,
    account_nonce: u64,
    call_data: Vec<u8>,
) -> String {
    let genesis_hash = encode_then_hex(&api.genesis_hash());
    // These numbers aren't SCALE encoded; their bytes are just converted to hex:
    let spec_version = to_hex(&api.runtime_version().spec_version.to_be_bytes());
    let transaction_version = to_hex(&api.runtime_version().transaction_version.to_be_bytes());
    let nonce = to_hex(&account_nonce.to_be_bytes());
    // If you construct a mortal transaction, then this block hash needs to correspond
    // to the block number passed to `Era::mortal()`.
    let mortality_checkpoint = encode_then_hex(&api.genesis_hash());
    let era = encode_then_hex(&Era::Immortal);
    let method = to_hex(call_data);
    let signed_extensions: Vec<String> = api
        .metadata()
        .extrinsic()
        .signed_extensions()
        .iter()
        .map(|e| e.identifier().to_string())
        .collect();
    let tip = encode_then_hex(&Compact(0u128));

    let payload = json!({
        "specVersion": spec_version,
        "transactionVersion": transaction_version,
        "address": account_address,
        "blockHash": mortality_checkpoint,
        "blockNumber": "0x00000000",
        "era": era,
        "genesisHash": genesis_hash,
        "method": method,
        "nonce": nonce,
        "signedExtensions": signed_extensions,
        "tip": tip,
        "version": 4,
    });

    payload.to_string()
}

pub async fn submit_wait_inblock_and_get_event(
    extrinsic: SubmittableExtrinsic<PolkadotConfig, OnlineClient<PolkadotConfig>>,
) -> Result<crate::tinkernet::system::events::ExtrinsicSuccess, anyhow::Error> {
    let events = extrinsic
        .submit_and_watch()
        .await?
        .wait_for_in_block()
        .await?
        .fetch_events()
        .await?;

    let events_str = format!("{:?}", &events);
    console_log!("{}", events_str);
    for event in events.find::<crate::tinkernet::system::events::ExtrinsicSuccess>() {
        console_log!("{:?}", event);
    }

    let core_created_event = events
        .find_first::<crate::tinkernet::inv4::events::CoreCreated>()?
        .unwrap();

    console_log!("core_created_event: {:#?}", core_created_event);

    let success = events.find_first::<crate::tinkernet::system::events::ExtrinsicSuccess>()?;
    success.ok_or(anyhow!("ExtrinsicSuccess not found in events"))
}

#[derive(Deserialize)]
pub struct SignatureResponse {
    pub signature: String,
}
