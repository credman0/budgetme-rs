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
use std::collections::{HashMap, HashSet};

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
#[derive(Derivative, Serialize, Deserialize, Clone, Debug)]
#[derivative(PartialEq)]
pub struct Data {
    pub version:Option<f32>,
    history:Vec<HistoryItem>,
    redo_stack:Vec<HistoryItem>,
    balance:f32,
    #[serde(default)]
    debt:f32,
    #[derivative(PartialEq="ignore")]
    last_updated:u64,
    pub rate:Option<f32>,
    #[serde(default)]
    cringe_factors:HashMap<String, f32>,
    #[serde(default)]
    synonyms:HashMap<String, HashSet<String>>
}

impl Data {
    pub fn new() -> Data {
        return Data {
            version:Some(*crate::DATA_VERSION),
            history:vec![],
            redo_stack:vec![],
            balance:10.,
            debt:0.,
            rate:Some(5.),
            last_updated:Local::now().timestamp_millis() as u64,
            cringe_factors:HashMap::new(),
            synonyms:HashMap::new()
        }
    }

    pub fn update(&mut self, rate:&f32) {
        let now = Local::now();
        let current = now.num_days_from_ce();
        let last = Local.timestamp_millis(self.last_updated as i64).num_days_from_ce();
        assert_eq!(current>=last, true);

        let elapsed = current-last;

        // The gains before any garnishes
        let mut net_gains = rate*elapsed as f32;
        if self.debt > 0. {
            if self.debt > net_gains/2. {
                self.debt-=net_gains/2.;
                net_gains /= 2.0;
            } else {
                net_gains -= self.debt;
                self.debt = 0.;
            }
        }
        self.balance = self.balance + net_gains;
        self.last_updated = now.timestamp_millis() as u64;
    }

    pub fn set_cringe(&mut self, keyword:&dyn AsRef<str>, factor:f32) {
        let keyword = &keyword.as_ref().to_ascii_lowercase();
        if self.has_synonyms(keyword) {
            let synonyms = self.get_synonyms(keyword);
            for synonym in synonyms {
                if self.cringe_factors.contains_key(&synonym) {
                    self.cringe_factors.insert(synonym.to_string(), factor);
                    return;
                }
            }
        }
        self.cringe_factors.insert(keyword.to_string(), factor);
    }
    
    pub fn get_cringiness(&self, keyword:&dyn AsRef<str>) -> f32 {
        let keyword = &keyword.as_ref().to_ascii_lowercase();
        if self.has_synonyms(keyword) {
            let synonyms = self.get_synonyms(keyword);
            for synonym in synonyms {
                if self.cringe_factors.contains_key(&synonym) {
                    return *self.cringe_factors.get(&synonym).unwrap();
                }
            }
        }
        return 1f32;

    }

    pub fn set_synonym(&mut self, first:&dyn AsRef<str>, second:&dyn AsRef<str>) {
        let first = first.as_ref().to_string().to_ascii_lowercase();
        let second = second.as_ref().to_string().to_ascii_lowercase();
        if !self.synonyms.contains_key(&first) {
            self.synonyms.insert(first.clone(), HashSet::new());
        }
        let set = self.synonyms.get_mut(&first).unwrap();
        set.insert(second.clone());

        if !self.synonyms.contains_key(&second) {
            self.synonyms.insert(second.clone(), HashSet::new());
        }
        let set = self.synonyms.get_mut(&second).unwrap();
        set.insert(first.clone());
    }

    pub fn get_synonyms<'a>(&self, key:&dyn AsRef<str>) -> HashSet<String>{
        let key = key.as_ref().to_string().to_ascii_lowercase();
        let mut res = self.synonyms.get(&key).unwrap().clone();
        res.insert(key);
        return res;
    }

    pub fn has_synonyms<'a>(&self, key:&dyn AsRef<str>) -> bool{
        let key = key.as_ref().to_string().to_ascii_lowercase();
        return self.synonyms.contains_key(&key);
    }

    /// The balance minus the debts
    pub fn total_balance(&self) -> f32{
        return self.balance - self.debt;
    }
}

// #[derive(Serialize, Deserialize)]
// struct History {
//     items
// }

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct HistoryItem {
    amount:f32,
    reason:String,
    #[serde(skip_serializing_if = "Option::is_none")]
    specific:Option<String>,
    time:u64
}

fn format_dollars(amount:&f32) -> String {
    let sign_string = if amount < &0. {"-"} else {""};
    let result = format!("{}${:.2}", sign_string, amount.abs());
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
        println!("{}: {} {} {}", date.format(format_str).to_string().blue().on_black(), format_dollars(&self.amount).bright_red().on_black(), self.reason.yellow().on_black(), 
            if self.specific.is_some() {
                format!("({})", self.specific.as_ref().unwrap())
            } else {
                "".to_string()
            }.yellow().on_black()
        );
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

    pub fn garnish(&mut self) {
        if self.data.balance >= 0. {
            println!("{}", "Balance is already non-negative".bright_red().on_black());
            return;
        }
        // Turn the balance into a "positive" debt
        self.data.debt -= self.data.balance;
        self.data.balance = 0.;
        self.print_balance();
    }

    pub fn spend(&mut self, amount:f32, reason:String, specific:Option<String>, loan:&bool) {
        if amount <= 0. {
            println!("{}", "Amount must be positive!".bright_red().on_black());
        }
        let amount_scaled = amount*self.data.get_cringiness(&reason.to_ascii_lowercase());
        // if self.data.cringe_factors.contains_key(&reason.to_ascii_lowercase()) {
        //     amount_scaled = self.data.cringe_factors[&reason.to_ascii_lowercase()]*amount;
        // } else {
        //     amount_scaled = amount; 
        // }
        let new_balance = self.data.balance-amount_scaled;
        if new_balance < 0. && !loan {
            println!("{}", "Request is over budget!".bright_red().on_black());
            println!("Balance: {}", format_dollars(&self.data.balance).bright_red().on_black());
        } else {
            let history_item = HistoryItem{amount:amount_scaled, reason:reason, specific:specific, time:Local::now().timestamp_millis() as u64};
            history_item.print();
            self.data.history.push(history_item);
            self.data.balance = new_balance;
            let balance_formatted = if new_balance<0. {format_dollars(&new_balance).bright_red().on_black()} else {format_dollars(&new_balance).green().on_black()};
            println!("Balance: {}", balance_formatted);
        }
    }
    
    pub fn set_cfg(&mut self, key:&CfgKey, values:&Vec<String>) {
        let value = values[0].clone();
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
            CfgKey::Cringe => {
                if values.len() != 2 {
                    panic!("Wrong number of arguments to cringe")
                }
                
                let cringe_keyword = values[0].clone();
                let cringe_factor = f32::from_str(&values[1]);

                self.data.set_cringe(&cringe_keyword, cringe_factor.unwrap());
            },
            CfgKey::Synonym => {

                if values.len() != 2 {
                    panic!("Wrong number of arguments to Synonym")
                }
                let first = values[0].clone();
                let second = values[1].clone();

                self.data.set_synonym(&first, &second);
            }
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
            CfgKey::Cringe => {
                println!("{:?}", &self.data.cringe_factors);
            },
            CfgKey::Synonym => {
                println!("{:?}", &self.data.synonyms);
            }

        }
    }

    pub fn print_rate(&self) {
        println!("Rate is {}", format_dollars(&self.data.rate.unwrap()).green().on_black());
    }

    pub fn print_balance(&self) {
        let balance_formatted = if self.data.balance<0. {format_dollars(&self.data.balance).bright_red().on_black()} else {format_dollars(&self.data.balance).green().on_black()};
        let debt_formatted = if self.data.debt>0. {format_dollars(&self.data.debt).yellow().on_black()} else {format_dollars(&self.data.debt).green().on_black()};
        println!("Balance:\t{}\nDebt:\t\t{}", balance_formatted, debt_formatted);
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
                if self.data.total_balance() == old_data_updated.total_balance() {
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
                if self.data.total_balance() == old_data_updated.total_balance() {
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
        if self.data.history == old_data_updated.history && self.data.total_balance() == old_data_updated.total_balance() {
            // updated cringe or something else, hope OK
            return true;
        }
        println!("Unknown verifcation failure: {:?} vs {:?}", &old_data_updated, &self.data);
        return false;
    }
}