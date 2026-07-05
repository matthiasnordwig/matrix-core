//! CRUD for `llm_endpoint_pools` + `llm_endpoint_pool_members`: named groups of
//! `llm_endpoints` that a caller can dispatch work across (see the app-side
//! load-balancer). `set_pool_members` is the single write path for membership
//! and is where the "at most one local (gguf) member" invariant is enforced —
//! only one on-device model can run at a time on a given device.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{CoreError, Database, Result};

fn row_to_pool(row: &Row<'_>) -> rusqlite::Result<LlmEndpointPool> {
    Ok(LlmEndpointPool {
        id: row.get("id")?,
        name: row.get("name")?,
        created_at: row.get("created_at")?,
    })
}

impl Database {
    pub fn create_pool(&self, p: &NewLlmEndpointPool) -> Result<LlmEndpointPool> {
        self.conn.execute(
            "INSERT INTO llm_endpoint_pools (name) VALUES (?1)",
            params![p.name],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.pool(id)?.expect("row just inserted must exist"))
    }

    pub fn pool(&self, id: i64) -> Result<Option<LlmEndpointPool>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM llm_endpoint_pools WHERE id = ?1",
                [id],
                row_to_pool,
            )
            .optional()?)
    }

    pub fn list_pools(&self) -> Result<Vec<LlmEndpointPool>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM llm_endpoint_pools ORDER BY name")?;
        let rows = stmt.query_map([], row_to_pool)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn rename_pool(&self, id: i64, name: &str) -> Result<LlmEndpointPool> {
        self.conn.execute(
            "UPDATE llm_endpoint_pools SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        self.pool(id)?
            .ok_or_else(|| CoreError::NotFound(format!("llm_endpoint_pool {id}")))
    }

    pub fn delete_pool(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM llm_endpoint_pools WHERE id = ?1", [id])?
            > 0)
    }

    /// Members of a pool, in the order they were assigned via `set_pool_members`.
    pub fn list_pool_members(&self, pool_id: i64) -> Result<Vec<LlmEndpoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.* FROM llm_endpoints e
             JOIN llm_endpoint_pool_members m ON m.endpoint_id = e.id
             WHERE m.pool_id = ?1
             ORDER BY m.position",
        )?;
        let rows = stmt.query_map([pool_id], super::registries::row_to_llm_endpoint)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_pools_with_members(&self) -> Result<Vec<LlmEndpointPoolWithMembers>> {
        self.list_pools()?
            .into_iter()
            .map(|pool| {
                let members = self.list_pool_members(pool.id)?;
                Ok(LlmEndpointPoolWithMembers { pool, members })
            })
            .collect()
    }

    /// Replace a pool's full member list atomically. This is the only write
    /// path for pool membership so the "at most one gguf member" invariant
    /// can't be bypassed by adding members one at a time. Endpoints are
    /// resolved and validated before any row is touched, so a rejected call
    /// (unknown id, or >1 gguf endpoint) leaves the existing membership intact.
    pub fn set_pool_members(&self, pool_id: i64, endpoint_ids: &[i64]) -> Result<Vec<LlmEndpoint>> {
        let mut resolved = Vec::with_capacity(endpoint_ids.len());
        for &id in endpoint_ids {
            let ep = self
                .llm_endpoint(id)?
                .ok_or_else(|| CoreError::NotFound(format!("llm_endpoint {id}")))?;
            resolved.push(ep);
        }
        let gguf_count = resolved.iter().filter(|e| e.provider == "gguf").count();
        if gguf_count > 1 {
            return Err(CoreError::InvalidPoolMembers(
                "a pool may contain at most one local (gguf) endpoint".into(),
            ));
        }

        self.begin_transaction()?;
        let write = (|| -> Result<()> {
            self.conn.execute(
                "DELETE FROM llm_endpoint_pool_members WHERE pool_id = ?1",
                params![pool_id],
            )?;
            for (position, id) in endpoint_ids.iter().enumerate() {
                self.conn.execute(
                    "INSERT INTO llm_endpoint_pool_members (pool_id, endpoint_id, position) VALUES (?1, ?2, ?3)",
                    params![pool_id, id, position as i64],
                )?;
            }
            Ok(())
        })();
        match write {
            Ok(()) => {
                self.commit_transaction()?;
                Ok(resolved)
            }
            Err(e) => {
                let _ = self.rollback_transaction();
                Err(e)
            }
        }
    }
}
