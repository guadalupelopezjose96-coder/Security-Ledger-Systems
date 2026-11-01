#![forbid(unsafe_code)]

use rand::Rng;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use uuid::Uuid;

const PG_SERIALIZATION_FAILURE_CODE: &str = "40001";
const PG_DEADLOCK_DETECTED_CODE: &str = "40P01";

#[derive(Debug, thiserror::Error)]
pub enum LedgerRepositoryError {
    #[error("Violación de restricción de base de datos: {0}")]
    ConstraintViolation(String),

    #[error("Límite de reintentos alcanzado ({max_attempts}) por conflicto de serialización")]
    MaxRetriesExceeded { max_attempts: u32 },

    #[error("Error de base de datos no recuperable: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct LedgerLinePayload {
    pub account_id: Uuid,
    pub direction: &'static str,
    pub amount_cents: i64,
}

#[derive(Debug, Clone)]
pub struct CreateEntryCommand {
    pub tenant_id: Uuid,
    pub transaction_code: String,
    pub description: String,
    pub lines: Vec<LedgerLinePayload>,
}

#[derive(Clone)]
pub struct LedgerRepository {
    pool: PgPool,
    max_retries: u32,
    base_backoff_ms: u64,
}

impl LedgerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            max_retries: 5,
            base_backoff_ms: 25,
        }
    }

    fn is_retryable_error(error: &sqlx::Error) -> bool {
        if let sqlx::Error::Database(ref db_err) = error {
            if let Some(code) = db_err.code() {
                return code == PG_SERIALIZATION_FAILURE_CODE || code == PG_DEADLOCK_DETECTED_CODE;
            }
        }
        false
    }

    pub async fn create_journal_entry(
        &self,
        command: CreateEntryCommand,
    ) -> Result<Uuid, LedgerRepositoryError> {
        let mut attempts = 0;

        loop {
            attempts += 1;

            match self.execute_serializable_transaction(&command).await {
                Ok(journal_entry_id) => return Ok(journal_entry_id),
                Err(err) => {
                    if Self::is_retryable_error(&err) {
                        if attempts >= self.max_retries {
                            tracing::error!(
                                transaction_code = %command.transaction_code,
                                attempts,
                                "Superado el límite de reintentos por 40001"
                            );
                            return Err(LedgerRepositoryError::MaxRetriesExceeded {
                                max_attempts: self.max_retries,
                            });
                        }

                        let factor = 2_u64.saturating_pow(attempts - 1);
                        let max_delay_ms = self.base_backoff_ms.saturating_mul(factor);
                        let sleep_duration_ms = {
                            let mut rng = rand::thread_rng();
                            rng.gen_range(self.base_backoff_ms..=max_delay_ms.max(self.base_backoff_ms + 1))
                        };

                        tracing::warn!(
                            attempt = attempts,
                            retry_in_ms = sleep_duration_ms,
                            transaction_code = %command.transaction_code,
                            "Conflicto de serialización 40001 detectado. Reintentando..."
                        );

                        tokio::time::sleep(Duration::from_millis(sleep_duration_ms)).await;
                        continue;
                    }

                    if let sqlx::Error::Database(ref db_err) = err {
                        if let Some(code) = db_err.code() {
                            if code == "23505" || code == "23514" {
                                return Err(LedgerRepositoryError::ConstraintViolation(
                                    db_err.message().to_string(),
                                ));
                            }
                        }
                    }

                    return Err(LedgerRepositoryError::Database(err));
                }
            }
        }
    }

    async fn execute_serializable_transaction(
        &self,
        cmd: &CreateEntryCommand,
    ) -> Result<Uuid, sqlx::Error> {
        let mut tx: Transaction<'_, Postgres> = self.pool.begin().await?;

        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;

        sqlx::query("SET LOCAL app.current_tenant_id = $1")
            .bind(cmd.tenant_id.to_string())
            .execute(&mut *tx)
            .await?;

        let journal_entry_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO journal_entries (id, tenant_id, transaction_code, description) VALUES ($1, $2, $3, $4)"
        )
        .bind(journal_entry_id)
        .bind(cmd.tenant_id)
        .bind(&cmd.transaction_code)
        .bind(&cmd.description)
        .execute(&mut *tx)
        .await?;

        for line in &cmd.lines {
            let line_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO ledger_lines (id, journal_entry_id, account_id, direction, amount_cents) VALUES ($1, $2, $3, $4::entry_direction, $5)"
            )
            .bind(line_id)
            .bind(journal_entry_id)
            .bind(line.account_id)
            .bind(line.direction)
            .bind(line.amount_cents)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(journal_entry_id)
    }
}
