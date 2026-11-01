#![forbid(unsafe_code)]

use sqlx::postgres::PgPoolOptions;
use sqlx::Row;
use std::env;
use std::sync::Arc;
use tokio::task::JoinSet;
use uuid::Uuid;

use sqlx_serializable_ledger::ledger_repository::{
    CreateEntryCommand, LedgerLinePayload, LedgerRepository,
};

#[tokio::test]
async fn test_concurrent_serializable_ledger_entries() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://test_user:test_secure_password@localhost:5432/app_test_db".to_string()
    });

    let pool = PgPoolOptions::new()
        .max_connections(25)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&database_url)
        .await?;

    let repository = Arc::new(LedgerRepository::new(pool.clone()));

    let test_tenant_id = Uuid::new_v4();
    let account_debit = Uuid::new_v4();
    let account_credit = Uuid::new_v4();

    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(test_tenant_id.to_string())
        .execute(&pool)
        .await?;

    sqlx::query(
        "INSERT INTO accounts (id, tenant_id, account_number, name) VALUES ($1, $2, '1000-CASH', 'Cash Assets'), ($3, $2, '4000-REV', 'Service Revenue')"
    )
    .bind(account_debit)
    .bind(test_tenant_id)
    .bind(account_credit)
    .execute(&pool)
    .await?;

    let concurrency_count = 20;
    let transfer_amount_cents = 1500;
    let mut join_set = JoinSet::new();

    for index in 0..concurrency_count {
        let repo = Arc::clone(&repository);
        let command = CreateEntryCommand {
            tenant_id: test_tenant_id,
            transaction_code: format!("TX-CONCURRENT-{}-{}", test_tenant_id.simple(), index),
            description: format!("Operación concurrente {}", index),
            lines: vec![
                LedgerLinePayload {
                    account_id: account_debit,
                    direction: "DEBIT",
                    amount_cents: transfer_amount_cents,
                },
                LedgerLinePayload {
                    account_id: account_credit,
                    direction: "CREDIT",
                    amount_cents: transfer_amount_cents,
                },
            ],
        };

        join_set.spawn(async move {
            repo.create_journal_entry(command).await
        });
    }

    let mut successful_transactions = Vec::new();
    while let Some(join_result) = join_set.join_next().await {
        let tx_result = join_result.map_err(|e| format!("Fallo en JoinHandle: {:?}", e))?;
        match tx_result {
            Ok(entry_id) => successful_transactions.push(entry_id),
            Err(err) => {
                return Err(format!(
                    "Transacción no recuperada de conflicto SSI: {:?}",
                    err
                )
                .into());
            }
        }
    }

    assert_eq!(
        successful_transactions.len(),
        concurrency_count,
        "Todas las transacciones concurrentes deben haberse liquidado"
    );

    let mut check_tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(test_tenant_id.to_string())
        .execute(&mut *check_tx)
        .await?;

    let row = sqlx::query(
        r#"
        SELECT 
            COALESCE(SUM(CASE WHEN direction = 'DEBIT' THEN amount_cents ELSE 0 END), 0)::BIGINT AS total_debits,
            COALESCE(SUM(CASE WHEN direction = 'CREDIT' THEN amount_cents ELSE 0 END), 0)::BIGINT AS total_credits
        FROM ledger_lines ll
        JOIN journal_entries je ON ll.journal_entry_id = je.id
        WHERE je.tenant_id = $1
        "#
    )
    .bind(test_tenant_id)
    .fetch_one(&mut *check_tx)
    .await?;

    let total_debits: i64 = row.try_get("total_debits")?;
    let total_credits: i64 = row.try_get("total_credits")?;
    let expected_total_cents = (concurrency_count as i64) * transfer_amount_cents;

    assert_eq!(
        total_debits, total_credits,
        "La suma neta contable DEBIT - CREDIT debe ser cero"
    );

    assert_eq!(
        total_debits, expected_total_cents,
        "El volumen liquidado total ({}) no coincide con el esperado ({})",
        total_debits, expected_total_cents
    );

    check_tx.commit().await?;

    Ok(())
}
