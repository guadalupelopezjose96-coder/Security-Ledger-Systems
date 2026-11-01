-- Extensiones requeridas para restricciones de exclusión temporal y UUIDs
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "btree_gist";

-- Tipo enumerado para dirección contable
CREATE TYPE entry_direction AS ENUM ('DEBIT', 'CREDIT');

-- Tabla de Cuentas Contables
CREATE TABLE accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    account_number VARCHAR(32) NOT NULL,
    name VARCHAR(128) NOT NULL,
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_tenant_account UNIQUE (tenant_id, account_number)
);

-- Tabla de Períodos Contables con exclusión temporal
CREATE TABLE accounting_periods (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    period_name VARCHAR(64) NOT NULL,
    validity_window TSTZRANGE NOT NULL,
    is_closed BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT excl_overlapping_periods EXCLUDE USING gist (
        tenant_id WITH =,
        validity_window WITH &&
    )
);

-- Cabecera de Asientos Contables
CREATE TABLE journal_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    transaction_code VARCHAR(64) NOT NULL UNIQUE,
    description TEXT NOT NULL,
    posted_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    is_posted BOOLEAN NOT NULL DEFAULT TRUE
);

-- Líneas de Asiento Contable (Valores enteros en centavos)
CREATE TABLE ledger_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    journal_entry_id UUID NOT NULL REFERENCES journal_entries(id) ON DELETE RESTRICT,
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    direction entry_direction NOT NULL,
    amount_cents BIGINT NOT NULL CHECK (amount_cents > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX idx_ledger_lines_entry ON ledger_lines(journal_entry_id);
CREATE INDEX idx_ledger_lines_account ON ledger_lines(account_id);

-- Función de Validación de Balance Contable (Suma Debits - Credits = 0)
CREATE OR REPLACE FUNCTION verify_journal_entry_balance()
RETURNS TRIGGER AS $$
DECLARE
    v_balance BIGINT;
    v_entry_id UUID;
BEGIN
    IF TG_OP = 'DELETE' THEN
        v_entry_id := OLD.journal_entry_id;
    ELSE
        v_entry_id := NEW.journal_entry_id;
    END IF;

    SELECT COALESCE(
        SUM(
            CASE 
                WHEN direction = 'DEBIT' THEN amount_cents 
                WHEN direction = 'CREDIT' THEN -amount_cents 
            END
        ), 0)
    INTO v_balance
    FROM ledger_lines
    WHERE journal_entry_id = v_entry_id;

    IF v_balance <> 0 THEN
        RAISE EXCEPTION 'Desbalance contable en el asiento %: suma neta es % centavos (debe ser 0)',
            v_entry_id, v_balance
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Trigger Diferido para Evaluación al Ejecutar COMMIT
CREATE CONSTRAINT TRIGGER trg_enforce_double_entry_balance
    AFTER INSERT OR UPDATE OR DELETE ON ledger_lines
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    EXECUTE FUNCTION verify_journal_entry_balance();

-- Seguridad a Nivel de Filas (Row-Level Security)
ALTER TABLE accounts ENABLE ROW LEVEL SECURITY;
ALTER TABLE accounting_periods ENABLE ROW LEVEL SECURITY;
ALTER TABLE journal_entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE ledger_lines ENABLE ROW LEVEL SECURITY;

CREATE POLICY tenant_isolation_accounts ON accounts
    USING (tenant_id = NULLIF(current_setting('app.current_tenant_id', true), '')::UUID);

CREATE POLICY tenant_isolation_periods ON accounting_periods
    USING (tenant_id = NULLIF(current_setting('app.current_tenant_id', true), '')::UUID);

CREATE POLICY tenant_isolation_journal ON journal_entries
    USING (tenant_id = NULLIF(current_setting('app.current_tenant_id', true), '')::UUID);

CREATE POLICY tenant_isolation_lines ON ledger_lines
    USING (journal_entry_id IN (
        SELECT id FROM journal_entries 
        WHERE tenant_id = NULLIF(current_setting('app.current_tenant_id', true), '')::UUID
    ));
