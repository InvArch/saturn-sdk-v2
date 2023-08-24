mod js_signer;
mod utils;

use std::str::FromStr;

use js_signer::SignatureResponse;
use sp_arithmetic::per_things::Perbill;
use subxt::{
    ext::{
        codec::{Decode, Encode},
        scale_value::{Composite, Value},
    },
    tx::Payload,
    utils::{AccountId32, MultiSignature},
    Error as SubxtError, OnlineClient, PolkadotConfig,
};
use wasm_bindgen::prelude::*;

#[subxt::subxt(
    runtime_metadata_path = "./tinkernet.scale",
    substitute_type(
        path = "sp_arithmetic::per_things::Perbill",
        with = "::subxt::utils::Static<sp_arithmetic::per_things::Perbill>"
    ),
    derive_for_all_types = "Clone"
)]
pub mod tinkernet {}

use tinkernet::runtime_types::pallet_inv4::fee_handling::FeeAsset;

#[wasm_bindgen]
#[derive(Debug)]
pub struct SaturnError(String);

impl SaturnError {
    pub fn inner(self) -> String {
        self.0
    }
}

impl From<SubxtError> for SaturnError {
    fn from(value: SubxtError) -> Self {
        SaturnError(format!("{:?}", value))
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub enum JsFeeAsset {
    TNKR,
    KSM,
}

impl From<JsFeeAsset> for FeeAsset {
    fn from(value: JsFeeAsset) -> Self {
        match value {
            JsFeeAsset::TNKR => FeeAsset::TNKR,
            JsFeeAsset::KSM => FeeAsset::KSM,
        }
    }
}

#[wasm_bindgen]
pub struct CoreCreationResult {
    core_id: u32,
}

#[wasm_bindgen]
pub struct Saturn {
    api: OnlineClient<PolkadotConfig>,
}

#[wasm_bindgen]
impl Saturn {
    #[wasm_bindgen(constructor)]
    pub async fn new(url: String) -> Result<Saturn, SaturnError> {
        utils::set_panic_hook();

        let api = OnlineClient::<PolkadotConfig>::from_url(url)
            .await
            .map_err(|e| SaturnError::from(e))?;

        return Ok(Self { api });
    }

    #[wasm_bindgen]
    pub fn create_core(
        &self,
        metadata: String,
        minimum_support: u32,
        required_approval: u32,
        fee_asset: JsFeeAsset,
    ) -> Call {
        let dcd = subxt::dynamic::tx(
            "INV4",
            "create_core",
            vec![
                (
                    "metadata",
                    Value::from_bytes(
                        tinkernet::runtime_types::bounded_collections::bounded_vec::BoundedVec(
                            metadata.as_bytes().to_vec(),
                        )
                        .encode(),
                    ),
                ),
                (
                    "minimum_support",
                    Value::unnamed_composite([Value::u128(
                        Perbill::from_parts(minimum_support).deconstruct() as u128,
                    )]),
                ),
                (
                    "required_approval",
                    Value::unnamed_composite([Value::u128(
                        Perbill::from_parts(required_approval).deconstruct() as u128,
                    )]),
                ),
                (
                    "creation_fee_asset",
                    Value::unnamed_variant(
                        match fee_asset {
                            JsFeeAsset::TNKR => "TNKR",
                            JsFeeAsset::KSM => "KSM",
                        },
                        [],
                    ),
                ),
            ],
        );

        Call {
            api: self.api.clone(),
            call: dcd.clone(),
        }
    }

    #[wasm_bindgen]
    pub async fn get_voting_balance(
        &self,
        core_id: u32,
        account: String,
    ) -> Result<String, SaturnError> {
        let account_id = AccountId32::from_str(&account).unwrap();

        let storage_query = tinkernet::storage()
            .core_assets()
            .accounts(account_id, core_id);

        let result = self
            .api
            .storage()
            .at_latest()
            .await
            .map_err(|e| SaturnError::from(e))?
            .fetch(&storage_query)
            .await
            .map_err(|e| SaturnError::from(e))?;

        return Ok(result.unwrap().free.to_string());
    }
}

#[wasm_bindgen]
pub struct Call {
    api: OnlineClient<PolkadotConfig>,
    call: Payload<Composite<()>>,
}

#[wasm_bindgen]
impl Call {
    #[wasm_bindgen]
    pub async fn sign_and_submit(
        &self,
        address: String,
        signer_function: js_sys::Function,
    ) -> Result<(), SaturnError> {
        let account_id = AccountId32::from_str(&address).map_err(|e| {
            utils::console_log!("rust account_id error: {:?}", e);
            SaturnError(e.to_string())
        })?;

        let call_data = self.api.tx().call_data(&self.call).map_err(|e| {
            utils::console_log!("rust call_data error: {:?}", e);
            SaturnError(String::from("could not encode call data"))
        })?;

        let account_nonce = self
            .api
            .tx()
            .account_nonce(&account_id)
            .await
            .map_err(|e| {
                utils::console_log!("rust account_nonce error: {:?}", e);
                SaturnError(String::from("Fetching account nonce failed"))
            })?;

        let payload =
            js_signer::generate_payload(&self.api, address, account_nonce, call_data).await;

        let this = JsValue::null();
        let signature_future = signer_function
            .call1(&this, &js_signer::json_parse(payload))
            .map_err(|e| {
                utils::console_log!("rust signature call1 error: {:?}", e);
                SaturnError(format!("{:?}", e))
            })?;

        let signature =
            wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&signature_future))
                .await
                .map_err(|e| {
                    utils::console_log!("rust signature await {:?}", e);
                    SaturnError(format!("rust signature await {:?}", e))
                })?;

        let signature_response: SignatureResponse = serde_wasm_bindgen::from_value(signature)
            .map_err(|_| SaturnError(String::from("Error deserializing SignatureResponse")))?;

        let signature = signature_response.signature;

        let signature = hex::decode(&signature[2..]).map_err(|e| SaturnError(e.to_string()))?;

        let multi_signature = MultiSignature::decode(&mut &signature[..]).map_err(|e| {
            utils::console_log!("rust multi_signature error: {:?}", e);
            SaturnError(String::from("MultiSignature Decoding"))
        })?;

        let partial_signed = self
            .api
            .tx()
            .create_partial_signed_with_nonce(&self.call, account_nonce, Default::default())
            .map_err(|e| {
                utils::console_log!("rust partial_signed error: {:?}", e);
                SaturnError(format!("PartialExtrinsic creation failed. Error: {:?}", e))
            })?;

        // Apply the signature
        let signed_extrinsic =
            partial_signed.sign_with_address_and_signature(&account_id.into(), &multi_signature);

        let result = js_signer::submit_wait_inblock_and_get_event(signed_extrinsic)
            .await
            .map_err(|e| {
                utils::console_log!("rust result error: {:?}", e);
                return SaturnError(e.to_string());
            })?;

        utils::console_log!("rust result: {:?}", result);

        Ok(())
    }
}

// #[wasm_bindgen]
// pub struct Call {
//     api: OnlineClient<PolkadotConfig>,
//     call_data: Vec<u8>,
// }

// impl Call {
//     pub async fn new(
//         api: OnlineClient<PolkadotConfig>,
//         call_data: Vec<u8>,
//     ) -> Result<Call, SaturnError> {
//         Ok(Self { api, call_data })
//     }
// }

// #[wasm_bindgen]
// impl Call {
//     #[wasm_bindgen]
//     pub async fn sign_and_submit(
//         &self,
//         address: String,
//         signer_function: js_sys::Function,
//     ) -> Result<(), SaturnError> {
//         let account_id = AccountId32::from_str(&address).unwrap();

//         // let call_data = &self
//         //     .api
//         //     .tx()
//         //     .call_data(&call)
//         //     .map_err(SaturnError(String::from("could not encode call data")))?;

//         let account_nonce = &self
//             .api
//             .tx()
//             .account_nonce(&account_id)
//             .await
//             .map_err(|_| SaturnError(String::from("Fetching account nonce failed")))?;

//         let payload =
//             js_signer::generate_payload(&self.api, account_id, account_nonce, &self.call_data)
//                 .map_err(|e| SaturnError(e.to_string()))
//                 .await?;

//         let signature = signer_function
//             .call1(&JsValue::null(), &JsValue::from(payload))
//             .map_err(|e| SaturnError(format!("{:?}", e)))?
//             .as_string()
//             .ok_or(SaturnError(String::from(
//                 "Error converting JsValue into String",
//             )))?;

//         let signature = hex::decode(&signature[2..]).map_err(|e| SaturnError(e.to_string()))?;

//         let multi_signature = MultiSignature::decode(&mut &signature[..])
//             .map_err(|_| SaturnError(String::from("MultiSignature Decoding")))?;

//         let partial_signed = self
//             .api
//             .tx()
//             .create_partial_signed_with_nonce(&self.call_data, account_nonce, Default::default())
//             .map_err(|e| {
//                 SaturnError(format!("PartialExtrinsic creation failed. Error: {:?}", e))
//             })?;

//         // Apply the signature
//         let signed_extrinsic =
//             partial_signed.sign_with_address_and_signature(&account_id.into(), &multi_signature);

//         let result =
//             js_signer::submit_wait_finalized_and_get_extrinsic_success_event(signed_extrinsic)
//                 .await
//                 .map_err(|e| {
//                     utils::console_log!("rust result error: {:?}", e);
//                     return SaturnError(e.to_string());
//                 })?;

//         utils::console_log!("rust result: {:?}", result);

//         Ok(())
//     }
// }
