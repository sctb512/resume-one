use gpui::{Context, PromptLevel, Window, actions};
use resume_one_updater::{Installation, Prepared, Release};

use crate::ResumeOne;

actions!(resume_one, [CheckUpdates]);

#[derive(Default)]
pub(super) struct UpdateUi {
    pub installation: Option<Installation>,
    busy: bool,
    available: Option<Release>,
    ready: Option<Prepared>,
}

impl UpdateUi {
    pub fn label(&self) -> &'static str {
        if self.busy {
            "正在检查或下载更新…"
        } else if self.ready.is_some() {
            "重启并安装更新…"
        } else if self.available.is_some() {
            "有可用更新…"
        } else {
            "检查更新…"
        }
    }
}

impl ResumeOne {
    pub(super) fn check_updates(
        &mut self,
        _: &CheckUpdates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_manager_open || self.files.busy {
            return;
        }
        self.files.open = false;
        cx.notify();
        self.request_update(true, window, cx);
    }

    pub(super) fn request_update(
        &mut self,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.updates.busy {
            return;
        }
        if interactive && self.updates.ready.is_some() {
            self.offer_restart(window, cx);
            return;
        }
        if interactive && self.updates.available.is_some() {
            self.offer_download(window, cx);
            return;
        }
        let Some(installation) = self.updates.installation.clone() else {
            if interactive {
                self.editor_warning(
                    "检查更新",
                    "自动更新适用于从 GitHub Releases 下载的带版本标记的应用包。",
                    window,
                    cx,
                );
            }
            return;
        };
        self.updates.busy = true;
        let version = installation.version();
        let job = cx
            .background_executor()
            .spawn(async move { installation.check() });
        cx.spawn_in(window, async move |this, cx| {
            let outcome = job.await;
            this.update_in(cx, |this, window, cx| {
                this.updates.busy = false;
                match outcome {
                    Ok(Some(release)) => {
                        this.updates.available = Some(release);
                        if interactive {
                            this.offer_download(window, cx);
                        }
                    }
                    Ok(None) if interactive => {
                        this.editor_warning("已是当前渠道的最新版本", &version, window, cx);
                    }
                    Err(error) if interactive => {
                        this.editor_warning("无法检查更新", &error.to_string(), window, cx);
                    }
                    _ => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn offer_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(release) = self.updates.available.as_ref() else {
            return;
        };
        let message = format!(
            "可从 GitHub Releases 下载 {}。下载后可选择何时重启。",
            release.tag
        );
        self.updates.busy = true;
        let choice = window.prompt(
            PromptLevel::Info,
            "有可用更新",
            Some(&message),
            &["稍后", "下载更新"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let approved = choice.await.ok() == Some(1);
            this.update_in(cx, |this, window, cx| {
                this.updates.busy = false;
                if approved {
                    this.download_update(window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn download_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(installation), Some(release)) = (
            self.updates.installation.clone(),
            self.updates.available.clone(),
        ) else {
            return;
        };
        self.updates.busy = true;
        let job = cx
            .background_executor()
            .spawn(async move { release.prepare(installation) });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            this.update_in(cx, |this, window, cx| {
                this.updates.busy = false;
                match result {
                    Ok(ready) => {
                        this.updates.ready = Some(ready);
                        if this.update_is_safe() {
                            this.offer_restart(window, cx);
                        }
                    }
                    Err(error) => {
                        this.editor_warning("无法下载或验证更新", &error.to_string(), window, cx)
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn update_is_safe(&self) -> bool {
        !self.files.busy
            && !self.export_busy
            && !self.theme_manager_open
            && !self.recovery_pending
            && self
                .editor
                .as_ref()
                .is_none_or(|editor| !editor.saving && !editor.buffer.is_dirty())
    }

    fn offer_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.update_is_safe() {
            self.editor_warning(
                "请先保存当前工作",
                "更新已下载。保存简历并结束当前操作后，可从打开菜单重新选择安装。",
                window,
                cx,
            );
            return;
        }
        if let Some(installation) = &self.updates.installation
            && (self
                .model
                .document_path
                .as_deref()
                .is_some_and(|path| installation.contains(path))
                || self
                    .model
                    .folder
                    .as_ref()
                    .is_some_and(|folder| installation.contains(&folder.path))
                || self
                    .theme_store
                    .as_ref()
                    .is_some_and(|store| installation.contains(&store.directory)))
        {
            self.editor_warning(
                "应用目录中有正在使用的文件",
                "请将简历、简历目录或主题目录移到应用目录之外，再安装更新。",
                window,
                cx,
            );
            return;
        }
        self.updates.busy = true;
        let choice = window.prompt(
            PromptLevel::Info,
            "更新已准备好",
            Some("简里将重启并安装更新。旧版保存在应用旁边的更新备份目录。"),
            &["稍后", "重启并安装"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let approved = choice.await.ok() == Some(1);
            this.update_in(cx, |this, window, cx| {
                this.updates.busy = false;
                if approved
                    && this.update_is_safe()
                    && this.confirm_close(window, cx)
                    && let Some(prepared) = this.updates.ready.take()
                {
                    match prepared.handoff(this.model.document_path.as_deref()) {
                        Ok(()) => window.remove_window(),
                        Err(error) => {
                            this.editor_warning("无法启动安装", &error.to_string(), window, cx)
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
