#![forbid(unsafe_code)]

use sqlx::postgres::PgPoolOptions;
use std::env;
use uuid::Uuid;

mod ledger_repository;
use ledger_repository::{CreateEntryCommand, LedgerLinePayload, LedgerRepository};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info,sqlx_serializable_ledger=debug")
        .init();

    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://test_user:test_secure_password@localhost:5432/app_test_db".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(&database_url)
        .await?;

    let repository = LedgerRepository::new(pool.clone());
    let tenant_id = Uuid::new_v4();
    let account_cash = Uuid::new_v4();
    let account_revenue = Uuid::new_v4();
    // Establecer el contexto del inquilino en la sesión de la conexión
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, false)")
        .bind(tenant_id.to_string())
        .execute(&pool)
        .await?;

    // Registrar las dos cuentas requeridas para satisfacer la clave foránea
    sqlx::query(
        r#"
        INSERT INTO accounts (id, tenant_id, account_number, name, currency, is_active)
        VALUES 
            ($1, $2, '1001', 'Caja General', 'USD', true),
            ($3, $2, '4001', 'Ingresos por Ventas', 'USD', true)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(account_cash)
    .bind(tenant_id)
    .bind(account_revenue)
    .execute(&pool)
    .await?;

    let command = CreateEntryCommand {
        tenant_id,
        transaction_code: format!("TX-{}", Uuid::new_v4().simple()),
        description: "Transacción contable de prueba".to_string(),
        lines: vec![
            LedgerLinePayload {
                account_id: account_cash,
                direction: "DEBIT",
                amount_cents: 2500,
            },
            LedgerLinePayload {
                account_id: account_revenue,
                direction: "CREDIT",
                amount_cents: 2500,
            },
        ],
    };

    match repository.create_journal_entry(command).await {
        Ok(id) => tracing::info!(journal_entry_id = %id, "Asiento registrado correctamente"),
        Err(e) => tracing::error!(error = %e, "Fallo al registrar asiento"),
    }

    Ok(())
}
