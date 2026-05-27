-- Idempotent grants for clients connecting via Docker port mapping (non-localhost host).
CREATE USER IF NOT EXISTS 'uaas'@'%' IDENTIFIED BY 'uaas-password';
CREATE USER IF NOT EXISTS 'maas'@'%' IDENTIFIED BY 'maas-password';

GRANT ALL PRIVILEGES ON uaas_db.* TO 'uaas'@'%';
GRANT ALL PRIVILEGES ON main_uaas_db.* TO 'maas'@'%';

FLUSH PRIVILEGES;
