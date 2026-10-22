//! Process-wide environment caches that TS keeps (env-api-keys.ts). These change the process
//! environment, so they live alone in this test binary and run as one test.

use pi_ai::env_api_keys::get_env_api_key;

#[test]
fn vertex_adc_presence_is_cached_for_the_process() {
    let home = std::env::temp_dir().join(format!("pi-ai-env-fidelity-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: this binary has a single test; no other thread reads the environment concurrently.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
        std::env::remove_var("GOOGLE_CLOUD_API_KEY");
        std::env::set_var("GOOGLE_CLOUD_PROJECT", "p");
        std::env::set_var("GOOGLE_CLOUD_LOCATION", "us-central1");
    }
    assert_eq!(get_env_api_key("google-vertex", None), None);
    // `cachedVertexAdcCredentialsExists`: credentials created later are not noticed.
    let adc = home.join(".config/gcloud/application_default_credentials.json");
    std::fs::create_dir_all(adc.parent().unwrap()).unwrap();
    std::fs::write(&adc, "{}").unwrap();
    assert_eq!(get_env_api_key("google-vertex", None), None);
    // An explicit scoped path is checked every time.
    let mut env = pi_ai::ProviderEnv::new();
    env.insert("GOOGLE_APPLICATION_CREDENTIALS".into(), adc.to_string_lossy().into_owned());
    assert_eq!(get_env_api_key("google-vertex", Some(&env)).as_deref(), Some("<authenticated>"));
    let _ = std::fs::remove_dir_all(&home);
}
