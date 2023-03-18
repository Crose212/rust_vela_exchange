use web3::Web3;
use web3::contract::Contract;
use web3::transports::WebSocket;
use web3::types::{H160, U256};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use std::time::Duration;
use reqwest::Client;
use serde_json::Value;

use crate::web3_interface::{Worker, get_data, get_closing_data, get_signatures, parse_order_id};

pub async fn read_private_keys(file_path: &str) -> Vec<String> {

    let file = File::open(file_path).unwrap();
    let reader = BufReader::new(file);
    let mut private_keys = Vec::new();

    for line in reader.lines() {

        let private_key = line.unwrap().to_string();
        private_keys.push(private_key);
    }
    private_keys
}

pub async fn read_addresses(file_path: &str) -> Vec<H160> {

    let file = File::open(file_path).unwrap();
    let reader = BufReader::new(file);
    let mut addresses = Vec::new();

    for line in reader.lines() {

        let line = line.unwrap();
        let address = H160::from_str(&line).unwrap();
        addresses.push(address);
    }
    addresses
}

pub async fn init_workers(private_keys: Vec<String>, addresses: Vec<H160>, contract: Contract<WebSocket>) -> Vec<Worker>{

    let mut workers = Vec::<Worker>::new();

    for i in 0..addresses.len() {
        workers.push(Worker { address: addresses[i], pkey: private_keys[i].clone(), pos_id: None, data: None, signed_transaction: None, hash: None })
    }

    let data1 = get_data(contract.clone(), true).await;
    let data2 = get_data(contract.clone(), false).await;

    for i in 0..workers.len()/2 {
        workers[i].data = Some(data1.clone()); 
    }

    for i in workers.len()/2..workers.len() {
        workers[i].data = Some(data2.clone());
    }
    workers
}

pub async fn get_ether_price() -> U256{

    let client = Client::new();
    let url = "https://app.vela.exchange/api/public";
    let json_data = r#"{"route": "pricing","action": "GET_PAIR_PRICE","payload": {"pair":"ETH/USD"}}"#;

    let response = client.post(url)
        .header("Content-Type", "application/json")
        .body(json_data)
        .send()
        .await
        .unwrap();

    let value: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    let price = value["price"].as_f64().unwrap();
    let u256 = price * 1000000000000000000000000000000_f64;
    let price = U256::from(u256.to_string().parse::<i128>().unwrap());

    price
}

pub async fn process_workers(mut workers: Vec<Worker>, contract: Contract<WebSocket>, web3s: Web3<WebSocket>) -> Vec<Worker>{



    for i in 0..workers.len()/2 {
        workers[i].data = Some(get_closing_data(workers[i].clone(), contract.clone(), true).await);
    }
    
    for i in workers.len()/2..workers.len() {
        workers[i].data = Some(get_closing_data(workers[i].clone(), contract.clone(), false).await);
    }

    let signed_workers = get_signatures(workers, contract.address(), web3s).await;

    signed_workers

}

pub async fn parse_orders(mut workers: Vec<Worker>, web3s: Web3<WebSocket>) -> Vec<Worker> {

    let curr_block = web3s.eth().block_number().await.unwrap();
    std::thread::sleep(Duration::from_millis(300000));

    loop {
        
        if web3s.eth().block_number().await.unwrap() != curr_block {
            println!("block mined");
            break;
        }
        std::thread::sleep(Duration::from_millis(2000));
    }

    for i in 0..workers.len() {
        let order_id = parse_order_id(web3s.clone(), workers[i].hash.unwrap()).await;
        workers[i].pos_id = Some(order_id);
    }
    
    workers
}

