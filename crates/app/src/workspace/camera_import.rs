//! Camera import destinations. Cloud imports stage originals until the upload
//! finishes; they never register the staging directory with the local gallery.
use super::*;
use anyhow::{ensure, Result};
use schist_cloud::Scope;
use schist_i18n::t;
use std::path::Path;

/// Captured when Import opens, so browsing elsewhere or signing into another
/// account while the camera downloads cannot redirect the upload.
#[derive(Clone)]
pub(super) struct CloudImportTarget {
    pub epoch: u64,
    pub scope: Scope,
}

impl CloudImportTarget {
    fn check(&self, cloud: &cloud::CloudState) -> Result<()> {
        ensure!(self.epoch == cloud.epoch, t("cloud.upload.cancelled"));
        ensure!(cloud.client.is_some(), t("cloud.error.sign_in_first"));
        Ok(())
    }

    pub fn destination(&self, cloud: &cloud::CloudState) -> Result<ImportDestination> {
        self.check(cloud)?;
        Ok(ImportDestination::Cloud {
            target: self.clone(),
            staging: Arc::new(
                tempfile::Builder::new()
                    .prefix("schist-import-")
                    .tempdir()?,
            ),
        })
    }

    pub fn label(&self, cloud: &cloud::CloudState) -> String {
        let name = match &self.scope {
            Scope::Folder { id, .. } => cloud.folders.iter().find(|f| &f.id == id).map(|f| &f.name),
            Scope::Bucket { id } => cloud.buckets.iter().find(|b| &b.id == id).map(|b| &b.name),
            Scope::Library => None,
        };
        match name {
            Some(name) => format!("{} / {name}", t("cloud.upload.prompt")),
            None => t("cloud.upload.prompt").into(),
        }
    }
}

#[derive(Clone)]
pub(super) enum ImportDestination {
    Local(PathBuf),
    Cloud {
        target: CloudImportTarget,
        staging: Arc<tempfile::TempDir>,
    },
}

impl ImportDestination {
    pub fn path(&self) -> &Path {
        match self {
            Self::Local(path) => path,
            Self::Cloud { staging, .. } => staging.path(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}

impl Workspace {
    pub(super) fn import_destination(
        &self,
        local: impl FnOnce() -> Result<PathBuf>,
    ) -> Result<ImportDestination> {
        if let Some(target) = &self.library.import_cloud {
            target.destination(&self.cloud)
        } else {
            Ok(ImportDestination::Local(local()?))
        }
    }

    /// Keep local indexing entirely on the local import path.
    pub(super) fn watch_import_destination(&mut self, dest: &ImportDestination) {
        if dest.is_local() && !self.library.folders.iter().any(|p| p == dest.path()) {
            self.library.folders.push(dest.path().to_path_buf());
            self.library.folders.sort();
            self.library.save();
        }
        self.library.open = true;
    }

    pub(super) fn upload_camera_import(
        &mut self,
        target: CloudImportTarget,
        staging: Arc<tempfile::TempDir>,
        failed: usize,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = target.check(&self.cloud) {
            self.cloud_error(error.to_string());
            return;
        }
        let (bucket, folder) = match target.scope {
            Scope::Bucket { id } => (Some(id), None),
            Scope::Folder { id, .. } => (None, Some(id)),
            Scope::Library => (None, None),
        };
        self.cloud_upload_local(
            bucket,
            folder,
            cloud::LocalUpload::Camera {
                directory: staging,
                failed,
            },
            cx,
        );
    }
}

#[cfg(test)]
mod cloud_lifecycle_tests {
    use super::*;

    #[test]
    fn camera_import_rejects_an_account_change_before_staging_or_upload() {
        let cloud = cloud::CloudState::default();
        let target = CloudImportTarget {
            epoch: cloud.epoch,
            scope: Scope::Library,
        };
        assert_eq!(
            target.check(&cloud).unwrap_err().to_string(),
            t("cloud.error.sign_in_first")
        );
        let mut changed_account = cloud;
        changed_account.epoch += 1;
        assert_eq!(
            target.check(&changed_account).unwrap_err().to_string(),
            t("cloud.upload.cancelled")
        );
        assert!(target.destination(&changed_account).is_err());
    }
}
