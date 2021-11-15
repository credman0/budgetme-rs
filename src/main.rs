#[macro_use]
extern crate lazy_static;

mod data;
mod datasources;

use std::{
    fs
};
use colored::*;
use tokio::{runtime};

use data::{Budget,Config,Data};

// (Buf) Uncomment these lines to have the output buffered, this can provide
// better performance but is not always intuitive behaviour.
// use std::io::BufWriter;

use structopt::StructOpt;
use structopt::clap::arg_enum;
lazy_static! {
    static ref DATA_VERSION:f32 = format!("{}", env!("CARGO_PKG_VERSION_MAJOR")).parse().unwrap();
}

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
    /// Print the spending history, most recent last
    List,
    /// Undo the most recent spend action
    Undo,
    /// Redo the most recent undo
    Redo,
    /// Spend some money from the active account balance
    Spend {
        /// Amount to spend, in dollars
        amount:f32,
        /// The category of spending
        reason:String,
        /// More specific description of spending
        specific:Option<String>,
        /// Allow spending beyond the current account balance
        #[structopt(short="o", long)]
        loan:bool
    },
    /// Reset the balance, but put half the rate towards repaying the debt until it is repaid
    Garnish,
    #[structopt(flatten)]
    CfgCommand(CfgCommand)
}


#[derive(StructOpt, Debug)]
enum CfgCommand {
    /// Set a configuration value
    Set {
        #[structopt(possible_values = &CfgKey::variants(), case_insensitive = true)]
        key:CfgKey,
        values:Vec<String>,
    },
    /// Get a current configuration value
    Get {
        #[structopt(possible_values = &CfgKey::variants(), case_insensitive = true)]
        key:CfgKey
    }   
}

arg_enum! {
    #[derive(Debug)]
    pub enum CfgKey {
        Rate,
        Path,
        AccessKey,
        SecretKey,
        BucketName,
        Region,
        Provider,
        Cringe,
        Synonym
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
    let data_provider = config.get_provider_factory().borrow().to_provider();
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
            Command::Garnish => budget.garnish(),
            Command::Spend{amount, reason, specific, loan} => budget.spend(amount,reason,specific,&loan),
            Command::CfgCommand(command) => match command {
                CfgCommand::Set{key, values} => budget.set_cfg(&key, &values),
                CfgCommand::Get{key} => budget.get_cfg(&key)
            },
        }
    }
    fs::create_dir_all(base_dir).unwrap();

    // recompute provider in case of changes in settings
    let data_provider = config.get_provider_factory().borrow().to_provider();
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