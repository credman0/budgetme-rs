mod data;
mod datasources;

use std::{
    fs
};
use colored::*;
use serde::{Deserialize, Serialize};
use tokio::{runtime};

use data::{Budget,Config,Data};
use datasources::{DataProvider,LocalDataProvider,AwsS3DataProviderFactory, DataProviderFactory};

// (Buf) Uncomment these lines to have the output buffered, this can provide
// better performance but is not always intuitive behaviour.
// use std::io::BufWriter;

use structopt::StructOpt;
use structopt::clap::arg_enum;

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


#[derive(StructOpt, Debug)]
enum CfgCommand {
    Set {
        #[structopt(possible_values = &CfgKey::variants(), case_insensitive = true)]
        key:CfgKey,
        value:String
    },
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
        Provider
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
    let data_provider:Box<dyn DataProvider> = config.get_provider_factory().to_provider();
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
    let data_provider:Box<dyn DataProvider> = config.get_provider_factory().to_provider();
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