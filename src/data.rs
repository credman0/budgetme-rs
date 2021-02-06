use std::{
    path::{PathBuf}
};

use chrono::prelude::*;
use colored::*;
use serde::{Deserialize, Serialize};
use derivative::Derivative;
use rusoto_core::Region;

use std::rc::Rc;
use std::cell::RefCell;
use std::str::FromStr;

use crate::{CfgKey};
use crate::datasources::{AwsS3DataProviderFactory, LocalDataProvider, DataProviderFactory};

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    data_source:Option<DataSource>,
    pub local_data_source:Option<Rc<RefCell<LocalDataProvider>>>,
    pub aws_data_source:Option<Rc<RefCell<AwsS3DataProviderFactory>>>,
    pub use_local:Option<bool>
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
enum DataSource {
    Local(LocalDataProvider),
    Aws(AwsS3DataProviderFactory)
}

impl Config {
    pub fn new() -> Config {
        return Config {data_source:None, local_data_source:None, aws_data_source:None, use_local:None};
    }

    pub fn get_provider_factory(&mut self) -> Rc<RefCell<dyn DataProviderFactory>> {
        self.convert_from_datasource();
        if self.use_local() {
            return self.get_local()
        } else {
            return self.get_aws();
        }
    }

    /// Old system used the datasource enum, but we want to stop that
    fn convert_from_datasource (&mut self) {
        if self.data_source.is_some() {
            match self.data_source.as_ref().unwrap() {
                DataSource::Local(local) => {
                    self.local_data_source = Some(Rc::new(RefCell::new(local.clone())));
                },
                DataSource::Aws(aws) => {
                    self.aws_data_source = Some(Rc::new(RefCell::new(aws.clone())));
                }
            }
            self.data_source = None;
        }
    }

    fn use_local(&mut self) -> bool {
        if self.use_local.is_some() {
            return self.use_local.unwrap();
        } else {
            return true;
        }
    }

    pub fn get_local(&mut self) -> Rc<RefCell<LocalDataProvider>> {
        self.convert_from_datasource();
        if self.local_data_source.is_none() {
            self.local_data_source = Some(Rc::new(RefCell::new(LocalDataProvider::new())));
        }
        return self.local_data_source.clone().unwrap();
    }

    pub fn get_aws(&mut self) -> Rc<RefCell<AwsS3DataProviderFactory>> {
        self.convert_from_datasource();
        if self.aws_data_source.is_none() {
            self.aws_data_source = Some(Rc::new(RefCell::new(AwsS3DataProviderFactory::new())));
        }
        return self.aws_data_source.clone().unwrap();
    }
}
#[derive(Derivative, Serialize, Deserialize, Clone)]
#[derivative(PartialEq)]
pub struct Data {
    history:Vec<HistoryItem>,
    redo_stack:Vec<HistoryItem>,
    balance:f32,
    #[derivative(PartialEq="ignore")]
    last_updated:u64,
    pub rate:Option<f32>,
}

impl Data {
    pub fn new() -> Data {
        return Data {
            history:vec![],
            redo_stack:vec![],
            balance:10.,
            rate:Some(5.),
            last_updated:Local::now().timestamp_millis() as u64
        }
    }

    pub fn update(&mut self, rate:&f32) {
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

#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub struct HistoryItem {
    amount:f32,
    reason:String,
    time:u64
}

fn format_dollars(amount:&f32) -> String {
    let sign_string = if amount < &0. {"-"} else {""};
    let result = format!("{}${:.2}", sign_string, amount);
    return result;
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
        println!("{}: {} {}", date.format(format_str).to_string().blue().on_black(), format_dollars(&self.amount).bright_red().on_black(), self.reason.yellow().on_black());
    }
}

pub struct Budget {
    pub config:Config,
    pub data:Data
}

impl Budget {
    pub fn list(&self) {
        for item in &self.data.history {
            item.print();
        }
        self.print_balance();
    }

    pub fn undo(&mut self) {
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

    pub fn redo(&mut self) {
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

    pub fn spend(&mut self, amount:f32, reason:String, loan:&bool) {
        if amount <= 0. {
            println!("{}", "Amount must be positive!".bright_red().on_black());
        }
        let new_balance = self.data.balance-amount;
        if new_balance < 0. && !loan {
            println!("{}", "Request is over budget!".bright_red().on_black());
            println!("Balance: {}", format_dollars(&self.data.balance).bright_red().on_black());
        } else {
            let history_item = HistoryItem{amount:amount, reason:reason, time:Local::now().timestamp_millis() as u64};
            history_item.print();
            self.data.history.push(history_item);
            self.data.balance = new_balance;
            let balance_formatted = if new_balance<0. {format_dollars(&new_balance).bright_red().on_black()} else {format_dollars(&new_balance).green().on_black()};
            println!("Balance: {}", balance_formatted);
        }
    }
    
    pub fn set_cfg(&mut self, key:&CfgKey, value:&String) {
        match key {
            CfgKey::Rate => {
                self.data.rate = Some(value.parse::<f32>().unwrap());
                self.print_rate();
            },
            CfgKey::Path => {
                let provider = Rc::clone(&self.config.get_local());
                let mut provider = provider.borrow_mut();
                provider.file_path = PathBuf::from(value);
                println!("Data path: {}", provider.file_path.as_os_str().to_string_lossy())
            },
            CfgKey::AccessKey => {
                let provider = Rc::clone(&self.config.get_aws());
                let mut provider = provider.borrow_mut();
                provider.access_key = value.clone();
                println!("Access key: {}", provider.access_key)

            },
            CfgKey::SecretKey => {
                let provider = Rc::clone(&self.config.get_aws());
                let mut provider = provider.borrow_mut();
                provider.bucket_name = value.clone();
                println!("Secret key: {}", provider.secret_access_key)

            },
            CfgKey::BucketName => {
                let provider = Rc::clone(&self.config.get_aws());
                let mut provider = provider.borrow_mut();
                provider.access_key = value.clone();
                println!("Bucket name: {}", provider.bucket_name)

            },
            CfgKey::Region => {
                let provider = Rc::clone(&self.config.get_aws());
                let mut provider = provider.borrow_mut();
                provider.region = Region::from_str(value.as_str()).expect("Invalid region");
                println!("Region: {:?}", provider.region)
            },
            CfgKey::Provider => {
                match value.trim().to_lowercase().as_str() {
                    "aws" => {
                        self.config.use_local = Some(false);
                        println!("Provider set to AWS");
                    },
                    "local" => {
                        self.config.use_local = Some(true);
                        println!("Provider set to local");
                    },
                    _=>{
                        panic!("Invalid provider \"{}\", valid are aws or local", value)
                    }
                }
            },
        }
    }

    pub fn get_cfg(&mut self, key:&CfgKey) {
        match key {
            CfgKey::Rate => {
                self.print_rate();
            },
            CfgKey::Path => {
                let provider = Rc::clone(&self.config.get_local());
                let provider = provider.borrow();
                println!("Data path: {}", provider.file_path.as_os_str().to_string_lossy())
            },
            CfgKey::AccessKey => {
                let provider = Rc::clone(&self.config.get_aws());
                let provider = provider.borrow();
                println!("Access key: {}", provider.access_key)

            },
            CfgKey::SecretKey => {
                let provider = Rc::clone(&self.config.get_aws());
                let provider = provider.borrow();
                println!("Secret key: {}", provider.secret_access_key)

            },
            CfgKey::BucketName => {
                let provider = Rc::clone(&self.config.get_aws());
                let provider = provider.borrow();
                println!("Bucket name: {}", provider.bucket_name)

            },
            CfgKey::Region => {
                let provider = Rc::clone(&self.config.get_aws());
                let provider = provider.borrow();
                println!("Region: {:?}", provider.region)
            },
            CfgKey::Provider => {
                if self.config.use_local() {
                    println!("Provider set to local");
                } else{
                    println!("Provider set to AWS");
                }
            },
        }
    }

    pub fn print_rate(&self) {
        println!("Rate is {}", format_dollars(&self.data.rate.unwrap()).green().on_black());
    }

    pub fn print_balance(&self) {
        let balance_formatted = if self.data.balance<0. {format_dollars(&self.data.balance).bright_red().on_black()} else {format_dollars(&self.data.balance).green().on_black()};
        println!("Balance: {}", balance_formatted);
    }

    pub fn verify_against(&self, old_data:Data) -> bool{
        let mut old_data_updated = old_data.clone();
        old_data_updated.rate = self.data.rate;
        old_data_updated.update(&old_data_updated.rate.unwrap());
        if self.data == old_data_updated {
            return true;
        }
        if (old_data_updated.history.len() as i32 - self.data.history.len() as i32).abs() > 2 || (old_data_updated.redo_stack.len() as i32 - self.data.redo_stack.len() as i32).abs() > 2 {
            // histories are too different
            println!("{}", "Histories diverge by more than one entry".red().on_black());
            return false;
        }
        if old_data_updated.history.len() > 0 && old_data_updated.history.len() > self.data.history.len() {
            if  &old_data_updated.history[..old_data_updated.history.len()-1] == &self.data.history[..] {
                // everything matches except we have one more entry in the old data, EG we must have undone something
                let last_item = old_data_updated.history.last().unwrap();
                old_data_updated.balance += last_item.amount;
                if self.data.balance == old_data_updated.balance {
                    return true;
                } else {
                    println!("{}", format!("Data missing entry but old data history does not match (expected {} but found {})", self.data.balance, old_data_updated.balance).red().on_black());
                    return false;
                }
                // // revert
                // old_data_updated.balance -= last_item.amount;
            } else {
                println!("{}", "Histories are incompatible".red().on_black());
                return false;
            }
        } else if self.data.history.len() > 0 && self.data.history.len() > old_data_updated.history.len() {
            if  &self.data.history[..self.data.history.len()-1] == &old_data_updated.history[..] {
                // everything matches except we have one more entry in the new data, EG we must have added something
                let last_item = self.data.history.last().unwrap();
                old_data_updated.balance -= last_item.amount;
                if self.data.balance == old_data_updated.balance {
                    return true;
                } else {
                    println!("{}", format!("Data has new entry but diverges from old data (expected {} but found {})", self.data.balance, old_data_updated.balance).red().on_black());
                    return false;
                }
                // // revert
                // old_data_updated.balance += last_item.amount;
            } else {
                println!("{}", "Histories are incompatible".red().on_black());
                return false;
            }
        }
        return false;
    }
}