//! End-to-end: create a bag in memory, then validate it.

use bagr_wasm::{
    bag_info::BagInfo,
    create::{create, CreateOptions},
    hash::Algorithm,
    io::MemoryBag,
    validate::validate,
};

#[tokio::test(flavor = "current_thread")]
async fn create_then_validate_md5_and_sha256() {
    let mut payload = MemoryBag::new();
    payload.insert("hello.txt", b"hello\n".to_vec());
    payload.insert("nested/world.bin", vec![0u8, 1, 2, 3, 4, 5]);

    let mut info = BagInfo::new();
    info.set("Source-Organization", "bagr-wasm tests");

    let mut bag = MemoryBag::new();
    create(
        &payload,
        &mut bag,
        CreateOptions {
            algorithms: vec![Algorithm::Md5, Algorithm::Sha256],
            bag_info: info,
            include_tag_manifests: true,
            bagging_date: Some("2026-05-26".into()),
            software_agent: None,
        },
    )
    .await
    .expect("create");

    assert!(bag.files.contains_key("bagit.txt"));
    assert!(bag.files.contains_key("manifest-md5.txt"));
    assert!(bag.files.contains_key("manifest-sha256.txt"));
    assert!(bag.files.contains_key("tagmanifest-md5.txt"));
    assert!(bag.files.contains_key("data/hello.txt"));
    assert!(bag.files.contains_key("data/nested/world.bin"));

    let report = validate(&bag).await.expect("validate");
    assert_eq!(report.payload_files, 2);
    assert_eq!(report.payload_octets, "hello\n".len() as u64 + 6);
    assert_eq!(report.payload_manifests.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn validation_catches_corruption() {
    let mut payload = MemoryBag::new();
    payload.insert("a.txt", b"contents".to_vec());

    let mut bag = MemoryBag::new();
    create(&payload, &mut bag, CreateOptions::default())
        .await
        .unwrap();

    // Tamper with the payload after the manifest is written.
    bag.files.insert("data/a.txt".to_string(), b"different".to_vec());

    let err = validate(&bag).await.expect_err("should fail");
    let msg = format!("{err}");
    assert!(msg.contains("checksum mismatch"), "got {msg}");
}

#[tokio::test(flavor = "current_thread")]
async fn detects_missing_payload_file() {
    let mut payload = MemoryBag::new();
    payload.insert("a.txt", b"contents".to_vec());

    let mut bag = MemoryBag::new();
    create(&payload, &mut bag, CreateOptions::default())
        .await
        .unwrap();

    bag.files.remove("data/a.txt");

    let err = validate(&bag).await.expect_err("should fail");
    assert!(matches!(
        err,
        bagr_wasm::BagError::Incomplete(_)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn fetch_held_file_via_custom_source() {
    use async_trait::async_trait;
    use bagr_wasm::io::{BagSource, FileReader};
    use bagr_wasm::BagResult;

    // Build a normal bag, then move one payload file out and add it to
    // fetch.txt. The custom source serves the moved file via fetch_url().
    let mut payload = MemoryBag::new();
    payload.insert("a.txt", b"contents".to_vec());
    payload.insert("b.txt", b"second\n".to_vec());
    let mut bag = MemoryBag::new();
    create(&payload, &mut bag, CreateOptions::default())
        .await
        .unwrap();

    let held_bytes = bag.files.remove("data/a.txt").unwrap();
    bag.files.insert(
        "fetch.txt".to_string(),
        b"http://example.org/a 8 data/a.txt\n".to_vec(),
    );

    struct WithFetch {
        bag: MemoryBag,
        held: Vec<u8>,
    }

    struct OneShot(Option<Vec<u8>>);
    #[async_trait(?Send)]
    impl FileReader for OneShot {
        async fn next_chunk(&mut self) -> BagResult<Option<Vec<u8>>> {
            Ok(self.0.take())
        }
    }

    #[async_trait(?Send)]
    impl BagSource for WithFetch {
        async fn list(&self) -> BagResult<Vec<String>> {
            self.bag.list().await
        }
        async fn open(&self, p: &str) -> BagResult<Box<dyn FileReader + '_>> {
            self.bag.open(p).await
        }
        async fn fetch_url(
            &self,
            url: &str,
        ) -> BagResult<Option<Box<dyn FileReader + '_>>> {
            assert_eq!(url, "http://example.org/a");
            Ok(Some(Box::new(OneShot(Some(self.held.clone())))))
        }
    }

    let src = WithFetch { bag, held: held_bytes };
    validate(&src).await.expect("held file should validate via fetch_url");
}

#[tokio::test(flavor = "current_thread")]
async fn held_file_without_fetch_handler_fails_clearly() {
    let mut payload = MemoryBag::new();
    payload.insert("a.txt", b"contents".to_vec());
    let mut bag = MemoryBag::new();
    create(&payload, &mut bag, CreateOptions::default())
        .await
        .unwrap();

    bag.files.remove("data/a.txt");
    bag.files.insert(
        "fetch.txt".to_string(),
        b"http://example.org/a 8 data/a.txt\n".to_vec(),
    );

    // Without fetch_url, validation should fail with a clear message.
    let err = validate(&bag).await.expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("fetch.txt"),
        "expected fetch-related error, got: {msg}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_malformed_fetch_url() {
    let mut payload = MemoryBag::new();
    payload.insert("a.txt", b"contents".to_vec());
    let mut bag = MemoryBag::new();
    create(&payload, &mut bag, CreateOptions::default())
        .await
        .unwrap();

    bag.files
        .insert("fetch.txt".into(), b"not-a-url 8 data/extra.txt\n".to_vec());
    let err = validate(&bag).await.expect_err("malformed URL should fail");
    assert!(matches!(err, bagr_wasm::BagError::MalformedFetch(_)));
}
