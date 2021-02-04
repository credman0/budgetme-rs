use std::{
    fs,
    path::{Path, PathBuf}
};
use chrono::prelude::*;
use colored::*;
use serde::{Deserialize, Serialize};
use derivative::Derivative;
use rusoto_core::{Region,credential::StaticProvider};
use rusoto_s3::{S3, S3Client, CreateBucketRequest, PutObjectRequest, GetObjectRequest};
use async_trait::async_trait;
use tokio::{runtime, io::AsyncReadExt};
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

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
    #[structopt(flatten)]
    CfgCommand(CfgCommand)
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
enum DataSource {
    Local(LocalDataProvider),
    Aws(AwsS3DataProviderFactory)
}

#[derive(StructOpt, Debug)]
enum CfgCommand {
    Set {
        #[structopt(subcommand)]
        key:CfgKey,
        value:String
    },
    Get {
        #[structopt(subcommand)]
        key:CfgKey
    }   
}

#[derive(StructOpt, Debug)]
enum CfgKey {
    Rate,
    Path,
    AccessKey,
    SecretKey,
    BucketName,
    Region,
    Provider
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
struct Config {
    data_source:Option<DataSource>
}

impl Config {
    fn new() -> Config {
        return Config {data_source:None};
    }
}
#[derive(Derivative, Serialize, Deserialize, Clone)]
#[derivative(PartialEq)]
struct Data {
    history:Vec<HistoryItem>,
    redo_stack:Vec<HistoryItem>,
    balance:f32,
    #[derivative(PartialEq="ignore")]
    last_updated:u64,
    rate:Option<f32>,
}

impl Data {
    fn new() -> Data {
        return Data {
            history:vec![],
            redo_stack:vec![],
            balance:10.,
            rate:Some(5.),
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

#[derive(Serialize, Deserialize, PartialEq, Clone)]
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
    
    fn set_cfg(&mut self, key:&CfgKey, value:&String) {
        if let CfgKey::Provider = key {
            match value.trim().to_lowercase().as_str() {
                "aws" => {
                    self.config.data_source = Some(DataSource::Aws(AwsS3DataProviderFactory::new()));
                },
                "local" => {
                    self.config.data_source = Some(DataSource::Local(LocalDataProvider::new()));
                },
                _=>{
                    panic!("Invalid provider \"{}\", valid are aws or local", value)
                }
            }
        } else if let CfgKey::Rate = key {
            self.data.rate = Some(value.parse::<f32>().unwrap());
            self.print_rate();
        } else {
            if let Some(DataSource::Aws(provider)) = &mut self.config.data_source {
                match key {
                    CfgKey::BucketName => {
                        provider.bucket_name = value.to_string();
                        println!("Bucket name: {}", provider.bucket_name)
                    },
                    CfgKey::AccessKey =>{
                        provider.access_key = value.to_string();
                        println!("Access key: {}", provider.access_key)
                    },
                    CfgKey::SecretKey =>{
                        provider.secret_access_key = value.to_string();
                        println!("Secret key: {}", provider.secret_access_key)
                    },
                    _=> {
                        println!("Invalid key for aws data provider: {:?}",key)
                    }
                }
            } else if let Some(DataSource::Local(provider)) = &mut self.config.data_source {
                match key {
                    CfgKey::Path =>{
                        if value.to_lowercase() == "none" {
                            provider.file_path = LocalDataProvider::new().file_path;
                        } else {
                            provider.file_path = PathBuf::from(value);
                        }
                        println!("Data path: {}", provider.file_path.as_os_str().to_string_lossy())
                    },
                    _=> {
                        println!("Invalid key for local data provider: {:?}",key)
                    }
                }
            }
        }
    }

    fn get_cfg(&self, key:&CfgKey) {
        if let Some(DataSource::Aws(provider)) = &self.config.data_source {
            panic!("Cannot set path on aws provider")
        } else if let Some(DataSource::Local(provider)) = &self.config.data_source {
            match key {
                CfgKey::Rate => self.print_rate(),
                CfgKey::Path => {
                    println!("Data path: {}", provider.file_path.as_os_str().to_string_lossy())
                },
                _=> {
                    println!("Invalid key for local data provider: {:?}",key)
                }
            }
        }
    }

    fn print_rate(&self) {
        println!("Rate is {}", format!("${:.2}", self.data.rate.unwrap()).green().on_black());
    }

    fn print_balance(&self) {
        let balance_formatted = if self.data.balance<0. {format!("${:.2}", &self.data.balance).bright_red().on_black()} else {format!("${:.2}", &self.data.balance).green().on_black()};
        println!("Balance: {}", balance_formatted);
    }

    fn verify_against(&self, old_data:Data) -> bool{
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

#[async_trait]
trait DataProvider {
    async fn get(&self) -> Option<Data>;
    async fn put(&self,data:&Data);
}

// serializable configuration that can produce a data provider
trait DataProviderFactory {
    fn to_provider(self) -> Box<dyn DataProvider>;
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
struct LocalDataProvider {
    file_path:PathBuf
}

#[async_trait]
impl DataProvider for LocalDataProvider {
    async fn get(&self) -> Option<Data> {
        if self.full_path().exists() {
            let data:Data = serde_json::from_str(&fs::read_to_string(&self.full_path()).unwrap()).unwrap();
            return Some(data);
        } else {
            return None;
        }
    }

    async fn put(&self, data:&Data) {
        fs::create_dir_all(&self.file_path).unwrap_or_default();
        fs::write(self.full_path(), serde_json::to_string(&data).unwrap()).unwrap();
    }
}

impl DataProviderFactory for LocalDataProvider {
    fn to_provider(self) -> Box<dyn DataProvider> {
        return Box::new(self);
    }
}

impl LocalDataProvider {
    fn from(file_path:PathBuf) -> LocalDataProvider {
        return LocalDataProvider{file_path:file_path}
    }
    fn new() -> LocalDataProvider {
        let path = String::from(dirs::config_dir().unwrap().to_str().unwrap());
        let data_path = Path::new(&path).join("budgetme");
        //let full_data_path = data_path.join("data.json");
        return LocalDataProvider{file_path:data_path}
    }
    fn full_path(&self) -> PathBuf{
        return self.file_path.join("data.json");
    }
}

struct AwsS3DataProvider {
    s3:S3Client,
    bucket_name:String
}

#[async_trait]
impl DataProvider for AwsS3DataProvider {
    async fn get(&self) -> Option<Data> {
        let get_obj_req = GetObjectRequest {
            bucket: self.bucket_name.clone(),
            key: "data.json".to_string(),
            ..Default::default()
        };
        let result = self.s3.get_object(get_obj_req).await;
        if result.is_err() {
            return None;
        } else {
            let stream = result.unwrap().body.unwrap();
            let mut buffer = String::new();
            stream.into_async_read().read_to_string(&mut buffer).await.unwrap();
            let data:Data = serde_json::from_str(&buffer).unwrap();
            return Some(data);
        }
    }

    async fn put(&self, data:&Data) {
        self.create_bucket().await;
        let contents:Vec<u8> = serde_json::to_string(&data).unwrap().as_bytes().to_vec();
        let put_request = PutObjectRequest {
            bucket: self.bucket_name.to_owned(),
            key: "data.json".to_string(),
            body: Some(contents.into()),
            ..Default::default()
        };
        self.s3
            .put_object(put_request)
            .await
            .expect("Failed to put data object");
    }
}

impl AwsS3DataProvider {
    async fn create_bucket(&self) {
        let create_bucket_req = CreateBucketRequest {
            bucket: self.bucket_name.clone(),
            ..Default::default()
        };
        self.s3
            .create_bucket(create_bucket_req)
            .await
            .expect("Failed to create test bucket");
    }
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
struct AwsS3DataProviderFactory {
    access_key:String,
    secret_access_key:String,
    bucket_name:String,
    region:Region,
}

impl DataProviderFactory for AwsS3DataProviderFactory {
    fn to_provider(self) -> Box<dyn DataProvider> {
        return Box::new(AwsS3DataProvider{bucket_name:self.bucket_name.clone(), 
            s3:S3Client::new_with(
                rusoto_core::request::HttpClient::new().expect("Failed to create HTTP client"),
                StaticProvider::new(self.access_key, self.secret_access_key, None, None),
                self.region.clone(),
            )}
        );
    }
}

impl AwsS3DataProviderFactory {
    fn new() -> AwsS3DataProviderFactory {
        return AwsS3DataProviderFactory{access_key:"".to_string(), secret_access_key:"".to_string(), bucket_name:AwsS3DataProviderFactory::generate_bucket_name(), region:Region::UsEast1}
    }

    fn generate_bucket_name() -> String {
        let rand_string:String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
        return format!("bucket-{}", rand_string.to_lowercase())
    }
}

#[cfg(not(target_os="windows"))]
fn prepare_virtual_terminal() {
}

#[cfg(target_os="windows")]
fn prepare_virtual_terminal() {
    control::set_virtual_terminal(true).unwrap();
}

fn main() {
    prepare_virtual_terminal();
    let args = Cli::from_args();
    let base_dir = dirs::config_dir().unwrap().join("budgetme");
    let config_path = dirs::config_dir().unwrap().join("budgetme").join("config.json");
    let mut config:Config;
    if config_path.exists() {
        config = serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    } else {
        config = Config::new();
    }
    // let data_path;
    // if config.data_path.is_some() {
    //     data_path = PathBuf::from((&config).data_path.as_ref().unwrap());
    // } else {
    //     let path = String::from(dirs::config_dir().unwrap().to_str().unwrap());
    //     data_path = Path::new(&path).join("budgetme");
    // }
    // let mut full_data_path = data_path.join("data.json");
    //let mut data_provider:LocalDataProvider = LocalDataProvider::new(full_data_path.clone());
    let data_provider:Box<dyn DataProvider>;
    if config.data_source.is_none() {
        config.data_source = Some(DataSource::Local(LocalDataProvider::new()));
    }
    let data_source = config.data_source.clone();
    match data_source.unwrap() {
        DataSource::Local(provider) => {
            data_provider = provider.to_provider();
        },
        DataSource::Aws(provider) => {
            data_provider = provider.to_provider();
        }
    }
    //let data_provider:&DataProvider = &*AwsS3DataProviderFactory {access_key:"AKIA5S65SRCS2XZIQ5FF".to_string(), secret_access_key:"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".to_string(), bucket_name:"budgetdfasdfasdfasdfasdfasdf".to_string(), region:Region::UsEast1}.to_provider();
    let maybe_data = data_provider.get();
    let mut data:Data = runtime::Runtime::new().unwrap().block_on(async {
        maybe_data.await.unwrap_or(Data::new())
    });
    if data.rate.is_none() {
        data.rate = Some(5.);
    }
    let mut budget = Budget {config:config.clone(), data:data};
    budget.data.update(&budget.data.rate.unwrap().clone());
    if args.command.is_none() {
        budget.print_balance();
    } else {
        match args.command.unwrap() {
            Command::List => budget.list(),
            Command::Undo => budget.undo(),
            Command::Redo => budget.redo(),
            Command::Spend{amount, reason, loan} => budget.spend(amount,reason,&loan),
            Command::CfgCommand(command) => match command {
                CfgCommand::Set{key, value} => budget.set_cfg(&key, &value),
                CfgCommand::Get{key} => budget.get_cfg(&key)
            },
        }
    }
    fs::create_dir_all(base_dir).unwrap();

    // recompute provider in case of changes in settings
    let data_provider:Box<dyn DataProvider>;
    let data_source = config.data_source.clone();
    if data_source.is_some() {
        match data_source.unwrap() {
            DataSource::Local(provider) => {
                data_provider = provider.to_provider();
            },
            DataSource::Aws(provider) => {
                data_provider = provider.to_provider();
            }
        }
    } else {
        // should be impossible at this point
        data_provider = Box::new(LocalDataProvider::new());
    }
    // if budget.config.data_path.is_some() {
    //     fs::create_dir_all((&budget).config.data_path.as_ref().unwrap()).unwrap();
    //     // update the full path because it might have changed during configuration
    //     full_data_path = PathBuf::from((&budget.config).data_path.as_ref().unwrap()).join("data.json");
    // }
    //data_provider.file_path = full_data_path;
    fs::write(&config_path, serde_json::to_string(&budget.config).unwrap()).unwrap();
    let maybe_old_data = data_provider.get();
    runtime::Runtime::new().unwrap().block_on(async {
        let old_data:Data = maybe_old_data.await.unwrap_or(Data::new());
        if budget.verify_against(old_data) {
            data_provider.put(&budget.data).await;
            //fs::write(&full_data_path, serde_json::to_string(&budget.data).unwrap()).unwrap();
        } else {
            println!("{}", "Refusing to overwrite unrelated histories".red().on_black());
        }
    });
}