// 本文件是 service_install_body.rs 的分片（第 3/3 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：停用宿主或指定实例的皮肤。

impl SkinService {
    /// 在串行操作锁内停用宿主（或指定实例）的皮肤：停止相关监视任务并清除页面注入，返回受影响页面数与停用后的状态。
    pub async fn uninstall(
        &self,
        host: SkinHostKind,
        instance_id: Option<&str>,
    ) -> Result<SkinStatus, AppError> {
        let _operation = self.operation.lock().await;
        let (_completion, _sender, _cancel) = self.begin_install_completion()?;
        let _runtime_mutation = self.begin_host_runtime_mutation(host);
        self.reap_watch_tasks().await?;
        let preferred_endpoint = self.verified_endpoint_hint(host)?;
        let cleanup_endpoint = match instance_id {
            Some(id) => {
                match resolve_host_instance_with_preferred(host, id, preferred_endpoint).await {
                    Ok(instance) => instance.debug_port.map(CdpEndpoint::new),
                    Err(error)
                        if replaces_all_host_runtimes(host)
                            && error.code == "skin.host_instance_changed" =>
                    {
                        None
                    }
                    Err(error) => return Err(error),
                }
            }
            None => None,
        };
        let target_key = instance_id.map(|id| runtime_instance_key(host, id));
        let tasks = {
            let mut runtime = self.runtime.lock().await;
            check_host_authority()?;
            take_uninstall_watch_tasks(&mut runtime, host, target_key.as_deref())
        };
        let had_tasks = !tasks.is_empty();
        let mut affected_pages = self.stop_watch_tasks(tasks).await?;
        if replaces_all_host_runtimes(host) {
            let endpoint = cleanup_endpoint.or(preferred_endpoint);
            let removed = if let Some(endpoint) = endpoint {
                remove_from_endpoint(host, endpoint).await?
            } else {
                0
            };
            affected_pages = affected_pages.saturating_add(removed);
            if removed == 0 {
                affected_pages =
                    affected_pages.saturating_add(remove_from_existing_endpoint(host).await?);
            }
        } else if !had_tasks {
            affected_pages = if let Some(endpoint) = cleanup_endpoint {
                remove_from_endpoint(host, endpoint).await?
            } else {
                remove_from_existing_endpoint(host).await?
            };
        }
        Ok(SkinStatus::stopped(affected_pages))
    }
}
