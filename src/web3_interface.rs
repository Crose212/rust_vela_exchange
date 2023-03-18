use std::str::FromStr;
use std::time::Duration;

use secp256k1::SecretKey;
use tokio::task;
use web3::contract::{Contract, tokens::Tokenize};
use web3::{Web3, transports::WebSocket};
use web3::types::{Address, Bytes, TransactionParameters, H160, U256, SignedTransaction, H256};
use ethabi::{Event, EventParam, ParamType, RawLog};

use crate::options::{get_ether_price};

#[derive(Clone, Debug)]
pub struct Worker {
    pub address: H160,
    pub pkey: String,
    pub pos_id: Option<U256>,
    pub data: Option<Vec<u8>>,
    pub signed_transaction: Option<SignedTransaction>,
    pub hash: Option<H256>
}

pub async fn get_contract(web3s: Web3<WebSocket>) -> Contract<WebSocket> {

    let contract_addr = Address::from_str("0x5957582F020301a2f732ad17a69aB2D8B2741241").unwrap();
    let contract = Contract::from_json(
        web3s.eth(),
        contract_addr,
        include_bytes!("abi.json")
    )
    .unwrap();

    contract
}

pub async fn get_data(contract: Contract<WebSocket>, is_long: bool) -> Vec<u8> {

    let price: U256 = U256::exp10(30) * 5; // 1 usdc * N, 5 usdc is miminum amount for vela

    let data = contract
        .abi()
        .function("newPositionOrder")
        .unwrap()
        .encode_input(
            &(
                Address::from_str("0xA6E249FFB81cF6f28aB021C3Bd97620283C7335f").unwrap(),
                is_long,
                u8::from_str("0").unwrap(),
                vec![get_ether_price().await,
                U256::from(250),
                price.clone(), 
                price * 25],       // leverage
                Address::from_str("0x0000000000000000000000000000000000000000").unwrap()
            )
                .into_tokens(),
        )
        .unwrap();
    data
}

pub async fn get_signatures(mut workers: Vec<Worker>, contract_addr: H160, web3s: Web3<WebSocket>) -> Vec<Worker> {

    let mut futures = vec![];

    for worker in workers.iter() {

        let contract_addr = contract_addr.clone();
        let web3s = web3s.clone();

        let future = task::spawn(sign(worker.clone(), web3s, contract_addr));

        futures.push(tokio::spawn(future));
        println!("Signed transaction for account: {:?}", &worker.address);
    }

    let signed_datas = futures::future::join_all(futures).await;
    for i in 0..signed_datas.len() {
        if let Ok(inner_result) = signed_datas[i].as_ref() {
            if let Ok(signed_tx) = inner_result.as_ref() {
                workers[i].signed_transaction = Some(signed_tx.clone());
            }
        }
    }
    workers
}

async fn sign(worker: Worker, web3s: Web3<WebSocket>, contract_addr: H160) -> SignedTransaction {

    let nonce = web3s.eth().transaction_count(worker.address.clone(), None).await.unwrap();
    let signable_data = worker.data.clone().unwrap();
    let pkey = worker.pkey.clone();
    
    let transaction_obj = TransactionParameters {
        nonce: Some(nonce),
        to: Some(contract_addr),
        value: U256::exp10(14) * 0, //0.0001 eth * N
        gas: U256::exp10(5) * 22, // 100_000 * N
        //gas_price: Some(U256::exp10(9) * 5),  // 1 gwei * N
        gas_price: Some(web3s.eth().gas_price().await.unwrap()),
        data: Bytes(signable_data),
        ..Default::default()
    };

    let secret = SecretKey::from_str(&pkey).unwrap();
    let signed_data = web3s
        .accounts()
        .sign_transaction(transaction_obj, &secret)
        .await
        .unwrap();
    signed_data
}

pub async fn send_transaction(mut worker: Worker, web3s: Web3<WebSocket>) -> Worker {

    let result = web3s
        .eth()
        .send_raw_transaction(worker.clone().signed_transaction.unwrap().raw_transaction)
        .await
        .unwrap();

    println!("Transaction sent for account {:?}, with hash: {:?};", worker.address, result);
    worker.hash = Some(result); 
    worker
    
}

async fn get_params_and_event() -> (Event, ethabi::ethereum_types::H256) {
    let params = vec![EventParam {
        name: "key".to_string(),
        kind: ParamType::FixedBytes(32),
        indexed: false
    },EventParam {
        name: "account".to_string(),
        kind: ParamType::Address,
        indexed: false
    },EventParam {
        name: "indexToken".to_string(),
        kind: ParamType::Address,
        indexed: false
    },EventParam {
        name: "isLong".to_string(),      // я не знаю почему это так работает, но я не могу поменять это на бул(выдаёт ошибку). Этот ивент парситься как posId
        kind: ParamType::Uint(256),
        indexed: false                   // upd: в аби всё перепутано местами
    },EventParam {
        name: "posId".to_string(),
        kind: ParamType::Uint(256),                             
        indexed: false
    },EventParam {
        name: "positionType".to_string(),
        kind: ParamType::Uint(256),
        indexed: false
    },EventParam {
        name: "orderStatus".to_string(),
        kind: ParamType::Uint(8),
        indexed: false
    },EventParam {
        name: "triggerData".to_string(),
        kind: ParamType::Uint(256),
        indexed: false
    }];

    let event = Event {
        name: "NewOrder".to_string(),
        inputs: params,
        anonymous: false
    };

    let ev_hash = event.signature();

    (event, ev_hash)
}

pub async fn parse_order_id(web3s: Web3<WebSocket>, hash:H256) -> U256 {
    let receipt = web3s.eth().transaction_receipt(hash).await.unwrap().unwrap();
    let (event, ev_hash)  = get_params_and_event().await;

    let log = receipt.logs.iter().find(|log| {
        log.topics.iter().find(|topic| topic == &&H256::from_str("0xe508fdc8bb11e26fd52e43d09c05ba1b7a778fe93ba8a3814b608aa29c3e6cdd").unwrap()).is_some()
    });

    let res=match log {
        Some(l) => {
            Some(event.parse_log(RawLog {
                topics: vec![ ev_hash ],
                data: l.data.clone().0
            }))
        },
        None => None
    };

    let position_id = res.unwrap().unwrap().params[3].value.to_owned().into_uint().unwrap();
    let pos = U256::from(position_id.as_u128());
    std::thread::sleep(Duration::from_millis(20000));
    pos
}

pub async fn get_closing_data(worker: Worker, contract: Contract<WebSocket>, is_long: bool) -> Vec<u8>{
        let data = contract
            .abi()
            .function("decreasePosition")
            .unwrap()
            .encode_input(
                &(
                    Address::from_str("0xA6E249FFB81cF6f28aB021C3Bd97620283C7335f").unwrap(),
                    U256::exp10(30) * 5,
                    is_long,
                    worker.pos_id.unwrap()
                )
                    .into_tokens(),
            )
            .unwrap();
        data
}