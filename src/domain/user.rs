use directories::ProjectDirs;
use regex::Regex;
use reqwest::{header, Client};
use reqwest_cookie_store::CookieStoreMutex;
use std::{path::Path, sync::Arc, time::Duration};

pub struct User {
    pub client: Client,
    cookie_store: Arc<CookieStoreMutex>,
}

impl User {
    pub async fn new(username: &str, password: &str) -> Result<Self, String> {
        let Ok(cookie_store) = Self::load_cookies() else {
            return Err("Error loading cookies from disk".to_string());
        };

        //TODO do better cookie checks, refresh if cookies expired
        if cookie_store.lock().unwrap().iter_any().next().is_some() {
            println!("Loaded session from file");
            return Ok(Self {
                client: Self::build_client(cookie_store.clone()),
                cookie_store,
            });
        }

        //let (client, auth_token) = Self::auth_user(
        let client = Self::auth_user(
            username.to_string(),
            password.to_string(),
            cookie_store.clone(),
        )
        .await;

        let user = Self {
            client,
            cookie_store,
        };

        Self::write_cookies(&user).expect("Failed to write cookies");

        Ok(user)
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
        println!("{:?}", login_response.status());
        println!("Successfully logged in\n");
        client
    }

    fn load_cookies() -> Result<Arc<CookieStoreMutex>, String> {
        if let Some(proj_dirs) = ProjectDirs::from("", "", env!("CARGO_PKG_NAME")) {
            let config_dir = proj_dirs.config_dir();
            let cookie_store = {
                if let Ok(file) = std::fs::File::open(Path::new(&config_dir.join("cookies.json")))
                    .map(std::io::BufReader::new)
                {
                    cookie_store::serde::json::load(file).unwrap()
                } else {
                    cookie_store::CookieStore::new()
                }
            };
            let cookie_store = CookieStoreMutex::new(cookie_store);
            Ok(Arc::new(cookie_store))
        } else {
            Err("Failed to open config directory".into())
        }
    }

    pub fn write_cookies(&self) -> Result<(), String> {
        //TODO more error handling here
        //TODO undupe getting config directory, maybe store in config
        if let Some(proj_dirs) = ProjectDirs::from("", "", env!("CARGO_PKG_NAME")) {
            let config_dir = proj_dirs.config_dir();
            let store = self.cookie_store.lock().unwrap();
            let mut writer = std::fs::File::create(Path::new(&config_dir.join("cookies.json")))
                .map(std::io::BufWriter::new)
                .unwrap();
            cookie_store::serde::json::save(&store, &mut writer).unwrap();
            Ok(())
        } else {
            Err("Failed to open config directory".into())
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

pub async fn get_user(username: Option<String>, password: Option<String>) -> Result<User, String> {
    if let (Some(username), Some(password)) = (&username, &password) {
        match User::new(username, password).await {
            Ok(user) => Ok(user),
            Err(error) => Err(format!("User Error {error}")),
        }
    } else {
        Err("Username or Password not provided in config file".to_string())
    }
}
