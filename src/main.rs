mod options;
mod web3_interface;

use options::{read_addresses, read_private_keys, init_workers, process_workers};
use web3_interface::{get_contract, get_signatures, send_transaction, Worker};
use web3::{transports::WebSocket, Web3, types::H160, contract::Contract};

use std::env;

use crate::options::parse_orders;

#[tokio::main]
async fn main() {

    dotenv::dotenv().ok();
    
    let websocket = WebSocket::new(&env::var("SOCKET").unwrap()).await.unwrap();
    let web3s = Web3::new(websocket);
    println!("connected to WebSocket, current block: {:?}", web3s.eth().block_number().await.unwrap());

    let private_keys = read_private_keys("./files/pkeys.txt").await;
    let addresses = read_addresses("./files/addresses.txt").await;
    println!("Loaded {:?} addresses", addresses.len());

    let contract = get_contract(web3s.clone()).await;
    loop {
        execute(private_keys.clone(), addresses.clone(), contract.clone(), web3s.clone()).await
    }
}

async fn execute(private_keys: Vec<String>, addresses: Vec<H160>, contract: Contract<WebSocket>, web3s: Web3<WebSocket>) {
    
    let workers = init_workers(private_keys, addresses, contract.clone()).await;

    let workers_with_positions = send_open_new_positions(workers, contract.clone(), web3s.clone()).await;

    let parsed_workers = parse_orders(workers_with_positions, web3s.clone()).await;

    
    close_positions(parsed_workers, contract, web3s).await;

}

async fn send_open_new_positions(workers: Vec<Worker>, contract: Contract<WebSocket>, web3s: Web3<WebSocket>) -> Vec<Worker> {

    let mut tasks = Vec::new();
    let workers = get_signatures(workers.clone(), contract.address().clone(), web3s.clone()).await;

    for worker in workers.iter() {

        let task = tokio::task::spawn(send_transaction(worker.clone(), web3s.clone()));
        tasks.push(task);
    }
    let workers = futures::future::join_all(tasks).await;

    let mut final_workers = Vec::new();
    for worker in workers.iter() {
        final_workers.push(worker.as_ref().unwrap().to_owned())
    }
    final_workers
}

async fn close_positions(workers: Vec<Worker>, contract: Contract<WebSocket>, web3s: Web3<WebSocket>) {

    let mut tasks = Vec::new();
    let workers = process_workers(workers, contract, web3s.clone()).await;

    for worker in workers.iter() {

        let task = tokio::task::spawn(send_transaction(worker.clone(), web3s.clone()));
        tasks.push(task);
    }
}