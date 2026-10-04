use regex::Regex;
use reqwest::{header, Client};
use reqwest_cookie_store::CookieStoreMutex;
use std::{env, path::PathBuf, sync::Arc, time::Duration};

use crate::config::Config;

pub struct User {
    pub client: Client,
    cookie_store: Arc<CookieStoreMutex>,
}

impl User {
    pub async fn new(config: &Config, login_path: String) -> Result<Self, String> {
        let cookie_store = match Self::load_cookies(config.get_cookies_path()) {
            Ok(store) => store,
            Err(e) => return Err(e),
        };

        //TODO do better cookie checks, refresh if cookies expired
        if cookie_store.lock().unwrap().iter_any().next().is_some() {
            println!("Loaded session from file");
            return Ok(Self {
                client: Self::build_client(cookie_store.clone()),
                cookie_store,
            });
        }

        dotenvy::from_path(PathBuf::from(login_path)).expect("Failed to load env file");
        let username = env::var("AO3_USERNAME").expect("Username missing from env file");
        let password = env::var("AO3_PASSWORD").expect("Password missing from env file");

        let client = Self::auth_user(username, password, cookie_store.clone()).await;

        let user = Self {
            client,
            cookie_store,
        };

        match Self::write_cookies(&user, config.get_cookies_path()) {
            Ok(_) => Ok(user),
            Err(e) => Err(e),
        }
    }

    async fn auth_user(
        username: String,
        password: String,
        cookie_store: Arc<CookieStoreMutex>,
    ) -> Client {
        println!("logging in");
        let client = Self::build_client(cookie_store);

        let html_content = client
            .get("https://archiveofourown.org/users/login")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let regex =
            Regex::new(r#"(id="new_user").+?("authenticity_token").+?"(?<token>.+?)""#).unwrap();
        let auth_token: String = regex.captures(&html_content).unwrap()["token"].to_owned();
        let form_data = [
            ("user[login]", username),
            ("user[password]", password),
            ("authenticity_token", auth_token),
        ];
        let login_response = client
            .post("https://archiveofourown.org/users/login")
            .form(&form_data)
            .send()
            .await
            .unwrap();
        // TODO do error checking here on the response status
        // Catch 429
        println!("{:?}", login_response.status());
        println!("Successfully logged in\n");
        client
    }

    fn load_cookies(cookie_path: PathBuf) -> Result<Arc<CookieStoreMutex>, String> {
        let cookie_store = {
            if let Ok(file) = std::fs::File::open(&cookie_path).map(std::io::BufReader::new) {
                cookie_store::serde::json::load(file).unwrap()
            } else {
                cookie_store::CookieStore::new()
            }
        };
        let cookie_store = CookieStoreMutex::new(cookie_store);
        Ok(Arc::new(cookie_store))
    }

    pub fn write_cookies(&self, cookie_path: PathBuf) -> Result<(), String> {
        let store = self.cookie_store.lock().unwrap();
        let mut writer = std::fs::File::create(&cookie_path).map(std::io::BufWriter::new);
        match &mut writer {
            Ok(writer) => {
                cookie_store::serde::json::save(&store, writer).unwrap();
                Ok(())
            }
            Err(e) => Err(format!("Error writing cookies: {}", e)),
        }
    }

    fn build_client(cookie_store: Arc<CookieStoreMutex>) -> Client {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
                .parse()
                .unwrap(),
        );
        headers.insert(header::ACCEPT_LANGUAGE, "en-US,en;q=0.5".parse().unwrap());
        headers.insert(header::REFERER, "https://google.com".parse().unwrap());

        Client::builder()
            .cookie_provider(Arc::clone(&cookie_store))
            //.user_agent("A2O4_Server/1.0")
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:153.0) Gecko/20100101 Firefox/153.0")
            .default_headers(headers)
            .use_native_tls()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
    }
}
