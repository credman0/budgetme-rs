use rusoto_core::{Region,credential::StaticProvider};
use rusoto_s3::{S3, S3Client, CreateBucketRequest, PutObjectRequest, GetObjectRequest};
use async_trait::async_trait;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use tokio::{io::AsyncReadExt};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use std::rc::Rc;

use crate::data::{Data};

#[async_trait]
pub trait DataProvider {
    async fn get(&self) -> Option<Data>;
    async fn put(&self,data:&Data);
}

// serializable configuration that can produce a data provider
pub trait DataProviderFactory {
    fn to_provider(&self) -> Rc<dyn DataProvider>;
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub struct LocalDataProvider {
    pub file_path:PathBuf
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
        fs::create_dir_all(&self.directory_path()).unwrap_or_default();
        fs::write(self.full_path(), serde_json::to_string(&data).unwrap()).unwrap();
    }
}

impl DataProviderFactory for LocalDataProvider {
    fn to_provider(&self) -> Rc<dyn DataProvider> {
        return Rc::new(self.clone());
    }
}

impl LocalDataProvider {
    // pub fn from(file_path:PathBuf) -> LocalDataProvider {
    //     return LocalDataProvider{file_path:file_path}
    // }
    pub fn new() -> LocalDataProvider {
        let path = String::from(dirs::config_dir().unwrap().to_str().unwrap());
        let data_path = Path::new(&path).join("budgetme");
        //let full_data_path = data_path.join("data.json");
        return LocalDataProvider{file_path:data_path}
    }

    /// directory path, which is the file path but expanding tilde to home
    fn directory_path(&self) -> PathBuf {
        let directory_path;
        if self.file_path.starts_with("~") {
            directory_path = PathBuf::from(dirs::home_dir().unwrap().join(self.file_path.strip_prefix("~").unwrap()));
        } else {
            directory_path = self.file_path.clone();
        }
        return directory_path;
    }

    fn full_path(&self) -> PathBuf{
        return self.directory_path().join("data.json");
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
pub struct AwsS3DataProviderFactory {
    pub access_key:String,
    pub secret_access_key:String,
    pub bucket_name:String,
    pub region:Region,
}

impl DataProviderFactory for AwsS3DataProviderFactory {
    fn to_provider(&self) -> Rc<dyn DataProvider> {
        return Rc::new(AwsS3DataProvider{bucket_name:self.bucket_name.clone(), 
            s3:S3Client::new_with(
                rusoto_core::request::HttpClient::new().expect("Failed to create HTTP client"),
                StaticProvider::new(self.access_key.clone(), self.secret_access_key.clone(), None, None),
                self.region.clone(),
            )}
        );
    }
}

impl AwsS3DataProviderFactory {
    pub fn new() -> AwsS3DataProviderFactory {
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