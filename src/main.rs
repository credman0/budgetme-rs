use std::{
    fs,
};
use chrono::prelude::*;
use colored::*;
use serde::{Deserialize, Serialize};

// (Buf) Uncomment these lines to have the output buffered, this can provide
// better performance but is not always intuitive behaviour.
// use std::io::BufWriter;

use structopt::StructOpt;

// Our CLI arguments. (help and version are automatically generated)
// Documentation on how to use:
// https://docs.rs/structopt/0.2.10/structopt/index.html#how-to-derivestructopt
#[derive(StructOpt, Debug)]
struct Cli {
    #[structopt(subcommand)]
    command:Option<Command>
}

#[derive(StructOpt, Debug)]
enum Command {
    List,
    Undo,
    Redo,
    Spend {
        amount:f32,
        reason:String,
        #[structopt(short="o", long)]
        loan:bool
    },
    Cfg {
        #[structopt(subcommand)]
        command:CfgCommand
    }
}

#[derive(StructOpt, Debug)]
enum CfgCommand {
    Set {
        key:String,
        value:String
    },
    Get {
        key:String
    }
}

#[derive(Serialize, Deserialize)]
struct Config {
    rate:f32,
}

impl Config {
    fn new() -> Config {
        return Config {rate:2.5};
    }
}

#[derive(Serialize, Deserialize)]
struct Data {
    history:Vec<HistoryItem>,
    redo_stack:Vec<HistoryItem>,
    balance:f32,
    last_updated:u64
}

impl Data {
    fn new() -> Data {
        return Data {
            history:vec![],
            redo_stack:vec![],
            balance:10.,
            last_updated:Local::now().timestamp_millis() as u64
        }
    }

    fn update(&mut self, rate:&f32) {
        let now = Local::now();
        let current = now.num_days_from_ce();
        let last = Local.timestamp_millis(self.last_updated as i64).num_days_from_ce();
        assert_eq!(current>=last, true);

        self.balance = self.balance + ((current-last) as f32)*rate;
        self.last_updated = now.timestamp_millis() as u64;
    }
}

// #[derive(Serialize, Deserialize)]
// struct History {
//     items
// }

#[derive(Serialize, Deserialize)]
struct HistoryItem {
    amount:f32,
    reason:String,
    time:u64
}

impl HistoryItem {
    fn print(&self) {
        let current_year = Local::now().year();
        let date = Local.timestamp_millis(self.time as i64);
        let item_year = date.year();
        let format_str;
        if current_year == item_year {
            format_str = "%b %d %I:%M%P"
        } else {
            format_str = "%b %d %Y %I:%M%P"
        }
        println!("{}: {} {}", date.format(format_str).to_string().blue().on_black(), format!("${:.2}", self.amount).bright_red().on_black(), self.reason.yellow().on_black());
    }
}

struct Budget {
    config:Config,
    data:Data
}

impl Budget {
    fn list(&self) {
        for item in &self.data.history {
            item.print();
        }
        self.print_balance();
    }

    fn undo(&mut self) {
        if self.data.history.len() == 0 {
            panic!("History is empty")
        }
        let last_item = self.data.history.pop().unwrap();
        last_item.print();
        let amount = last_item.amount;
        self.data.balance += amount;
        self.data.redo_stack.push(last_item);
        self.print_balance();
    }

    fn redo(&mut self) {
        if self.data.redo_stack.len() == 0 {
            panic!("Redo stack is empty")
        }
        let last_item = self.data.redo_stack.pop().unwrap();
        last_item.print();
        let amount = last_item.amount;
        self.data.balance -= amount;
        self.data.history.push(last_item);
        self.print_balance();
    }

    fn spend(&mut self, amount:f32, reason:String, loan:&bool) {
        if amount <= 0. {
            println!("{}", "Amount must be positive!".bright_red().on_black());
        }
        let new_balance = self.data.balance-amount;
        if new_balance < 0. && !loan {
            println!("{}", "Request is over budget!".bright_red().on_black());
            println!("Balance: {}", format!("${:.2}", &self.data.balance).bright_red().on_black());
        } else {
            let history_item = HistoryItem{amount:amount, reason:reason, time:Local::now().timestamp_millis() as u64};
            history_item.print();
            self.data.history.push(history_item);
            self.data.balance = new_balance;
            let balance_formatted = if new_balance<0. {format!("${:.2}", new_balance).bright_red().on_black()} else {format!("${:.2}", new_balance).green().on_black()};
            println!("Balance: {}", balance_formatted);
        }
    }
    
    fn set_cfg(&mut self, key:&String, value:&String) {
        match key.to_lowercase().as_str() {
            "rate" => {
                self.config.rate = value.parse::<f32>().unwrap();
                self.print_rate();
            },
            _ => panic!(format!("Unrecognized cfg key: {}", key))
        }
    }

    fn get_cfg(&self, key:&String) {
        match key.to_lowercase().as_str() {
            "rate" => self.print_rate(),
            _ => panic!(format!("Unrecognized cfg key: {}", key))
        }
    }

    fn print_rate(&self) {
        println!("rate is {}", format!("${:.2}", self.config.rate).green().on_black());
    }

    fn print_balance(&self) {
        let balance_formatted = if self.data.balance<0. {format!("${:.2}", &self.data.balance).bright_red().on_black()} else {format!("${:.2}", &self.data.balance).green().on_black()};
        println!("Balance: {}", balance_formatted);
    }
}

fn main() {
    let args = Cli::from_args();
    let base_dir = dirs::config_dir().unwrap().join("budgetme/");
    let config_path = dirs::config_dir().unwrap().join("budgetme/config.json");
    let data_path = dirs::config_dir().unwrap().join("budgetme/data.json");
    let config:Config;
    if config_path.exists() {
        config = serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    } else {
        config = Config::new();
    }
    let data:Data;
    if data_path.exists() {
        data = serde_json::from_str(&fs::read_to_string(&data_path).unwrap()).unwrap();
    } else {
        data = Data::new();
    }
    let mut budget = Budget {config:config, data:data};
    budget.data.update(&budget.config.rate);
    if args.command.is_none() {
        budget.print_balance();
    } else {
        match args.command.unwrap() {
            Command::List => budget.list(),
            Command::Undo => budget.undo(),
            Command::Redo => budget.redo(),
            Command::Spend{amount, reason, loan} => budget.spend(amount,reason,&loan),
            Command::Cfg{command} => match command {
                CfgCommand::Set{key, value} => budget.set_cfg(&key, &value),
                CfgCommand::Get{key} => budget.get_cfg(&key)
            },
        }
    }
    fs::create_dir_all(base_dir).unwrap();
    fs::write(&config_path, serde_json::to_string(&budget.config).unwrap()).unwrap();
    fs::write(&data_path, serde_json::to_string(&budget.data).unwrap()).unwrap();
}