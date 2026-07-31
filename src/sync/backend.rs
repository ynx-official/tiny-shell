use async_trait::async_trait;

use super::{
    S3Config, SyncBackendCredentials, SyncBackendKind, SyncFailure, SyncOperationResult, UploadMode,
};

#[async_trait]
pub(crate) trait SyncBackend: Send + Sync {
    fn kind(&self) -> SyncBackendKind;
    async fn upload(
        &self,
        credentials: &SyncBackendCredentials,
        body: Vec<u8>,
        mode: UploadMode,
    ) -> SyncOperationResult<Option<String>>;
    async fn download(
        &self,
        credentials: &SyncBackendCredentials,
    ) -> SyncOperationResult<(Vec<u8>, Option<String>)>;
}

pub(crate) struct WebDavBackend;
pub(crate) struct S3Backend;

#[async_trait]
impl SyncBackend for WebDavBackend {
    fn kind(&self) -> SyncBackendKind {
        SyncBackendKind::WebDav
    }

    async fn upload(
        &self,
        credentials: &SyncBackendCredentials,
        body: Vec<u8>,
        mode: UploadMode,
    ) -> SyncOperationResult<Option<String>> {
        let SyncBackendCredentials::WebDav {
            endpoint,
            username,
            password,
        } = credentials
        else {
            return Err(SyncFailure::other(
                Some(self.kind()),
                "invalid WebDAV credentials",
            ));
        };
        super::upload_webdav(endpoint, username, password, body, mode).await
    }

    async fn download(
        &self,
        credentials: &SyncBackendCredentials,
    ) -> SyncOperationResult<(Vec<u8>, Option<String>)> {
        let SyncBackendCredentials::WebDav {
            endpoint,
            username,
            password,
        } = credentials
        else {
            return Err(SyncFailure::other(
                Some(self.kind()),
                "invalid WebDAV credentials",
            ));
        };
        super::download_webdav(endpoint, username, password).await
    }
}

#[async_trait]
impl SyncBackend for S3Backend {
    fn kind(&self) -> SyncBackendKind {
        SyncBackendKind::S3
    }

    async fn upload(
        &self,
        credentials: &SyncBackendCredentials,
        body: Vec<u8>,
        mode: UploadMode,
    ) -> SyncOperationResult<Option<String>> {
        super::upload_s3(&s3_config(credentials, self.kind())?, body, mode).await
    }

    async fn download(
        &self,
        credentials: &SyncBackendCredentials,
    ) -> SyncOperationResult<(Vec<u8>, Option<String>)> {
        super::download_s3(&s3_config(credentials, self.kind())?).await
    }
}

fn s3_config(
    credentials: &SyncBackendCredentials,
    kind: SyncBackendKind,
) -> SyncOperationResult<S3Config> {
    let SyncBackendCredentials::S3 {
        endpoint,
        region,
        bucket,
        object_key,
        access_key,
        secret_key,
        session_token,
    } = credentials
    else {
        return Err(SyncFailure::other(Some(kind), "invalid S3 credentials"));
    };
    Ok(S3Config {
        endpoint: endpoint.clone(),
        region: region.clone(),
        bucket: bucket.clone(),
        object_key: object_key.clone(),
        access_key: access_key.clone(),
        secret_key: secret_key.clone(),
        session_token: session_token.clone(),
    })
}

pub(crate) fn for_credentials(credentials: &SyncBackendCredentials) -> &'static dyn SyncBackend {
    static WEBDAV: WebDavBackend = WebDavBackend;
    static S3: S3Backend = S3Backend;
    match credentials {
        SyncBackendCredentials::WebDav { .. } => &WEBDAV,
        SyncBackendCredentials::S3 { .. } => &S3,
    }
}
