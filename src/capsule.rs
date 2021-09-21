struct Configuration {
    a: u64
}

struct CachingBackend {}

#[derive(Default)]
struct Bundle {}

struct Capsule<'a> {
    config: &'a Configuration,
    caching_backend: &'a CachingBackend,
    key: Option<String>,
    cacheable_bundle: Option<Bundle>
}

impl<'a> Capsule<'a> {
    fn new(caching_backend: &'a CachingBackend,
           config: &'a Configuration) -> Self {
        Self {
            config: config,
            caching_backend,
            key: None,
            cacheable_bundle: None
        }
    }
}
