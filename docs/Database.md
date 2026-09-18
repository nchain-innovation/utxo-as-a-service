# Database

> **Two engines, for now.** The Rust indexer reads and writes **PostgreSQL**;
> that is the first section below. The Python REST API has not been moved yet
> and still reads **MariaDB**, which nothing writes to any more — so it serves
> whatever was last written there. The MariaDB section is kept for that reason
> and goes away with the Python data layer.

## Schema and migrations (PostgreSQL)

The schema lives in `rust/migrations/`, as `V{n}__{name}.sql`, one object per
file. It is **not** created by the application and **not** in
`init_database/`, which is a change from how this service has always worked.

### Why not in the application

Until this change, all eight tables and six indexes were created by Rust at
startup, each guarded by a query against `INFORMATION_SCHEMA` followed by a
bare `CREATE TABLE`. Three problems, none of which is fixable while the
schema lives in code:

* the check and the create are separate statements, so two instances starting
  together race;
* the existence probe does not filter by schema, so a same-named table anywhere
  visible suppresses creation;
* each core table had three or four definitions — production code, test code,
  CI heredocs — which had already drifted from one another.

### Why not in `init_database/`

Anything mounted at `/docker-entrypoint-initdb.d/` runs **only when the
container's data directory is empty**. It cannot evolve a database that already
exists, so it can create a schema once and never change it. That directory now
holds roles and databases only.

### Applying them

```bash
uaas migrate postgresql://uaas:uaas-password@localhost:5433/uaas_db
```

Or set `UAAS_POSTGRES_URL` and run `uaas migrate`. Safe to repeat: applied
migrations are skipped. The files are embedded in the binary at compile time,
so the published image carries no `psql` and no `.sql` files.

### What it refuses to do

| Situation | Behaviour |
|---|---|
| A migration file edited after it was applied | Refused, naming the file. An applied migration is history; add a new one instead. |
| The database records a migration this build does not have | Refused. The binary has been rolled back without its schema. |
| The service starts against a schema at the wrong version | Refused at startup, naming both versions, rather than failing later at a query. |

Each migration and its bookkeeping row commit in one transaction. PostgreSQL's
DDL is transactional, so a migration that fails part-way leaves nothing behind
— no half-created table and no row claiming success. This is the reason the
runner is ~150 lines rather than a dependency: the manual-repair procedure a
MySQL-family runner needs does not exist here, because MySQL-family DDL commits
implicitly.

### Adding a migration

1. Add `rust/migrations/V{n}__{name}.sql` with the next number.
2. Register it in `MIGRATIONS` in `rust/src/migrate.rs` and bump
   `EXPECTED_VERSION`.
3. `cargo test --lib migrate` — `mig01` fails if the file and the list disagree.

Never edit a file that has already been applied anywhere.

---

## MariaDB (Python REST API only)

Nothing in the Rust service speaks to this any more. It remains because the
Python REST API reads it, and it is removed when that moves to PostgreSQL.

This section describes the commands to set up and run MySQL in a Docker image
by hand; `docker-compose up` does all of it for you.

These steps are taken from  https://bitbucket.stressedsharks.com/projects/SDL/repos/utxo-identity/browse/UsersDB/dbschema?at=refs%2Fheads%2Fadd_tx

## Step 1 - Download and Start the MySQL Docker Image
1. Install Docker, if not already present
2. Download MySQL image:
```bash
docker pull mysql/mysql-server:latest
```
3. Start the Docker container for MySQL:
```bash
docker run --name=my-sql --publish 3306:3306 -d mysql/mysql-server
```
4. This can be checked using `docker ps`
```bash
docker ps  
CONTAINER ID   IMAGE                COMMAND                  CREATED       STATUS                 PORTS                                     NAMES
03499a1a60ac   mysql/mysql-server   "/entrypoint.sh mysq…"   2 hours ago   Up 2 hours (healthy)   0.0.0.0:3306->3306/tcp, 33060-33061/tcp   my-sql
```
## Step 2 - Configure the Database
When the database server initalises for the first time, the root user is created but expired. The user must connect to the docker instance & use the 'mysql' commandline tool reset the password.

1. Copy the root password the server created:
```bash
docker logs my-sql
...
[Entrypoint] GENERATED ROOT PASSWORD: @3Y5t/HqT;_h8O1A*y9@YmA7,pp0n0a:
...
```
2. Connect to the running MySQL container
```bash
docker exec -it my-sql bash
bash-4.4#
```
3. Connect to the server Set new root password

Use the copied password when prompted.

```sql
bash-4.4# mysql -u root -p
Enter password:
Welcome to the MySQL monitor.  Commands end with ; or \g.
Your MySQL connection id is 61
Server version: 8.0.28

Copyright (c) 2000, 2022, Oracle and/or its affiliates.

mysql> ALTER USER 'root'@'localhost' IDENTIFIED BY 'root-password';
Query OK, 0 rows affected (0.01 sec)
```
Then creating the following `root` users:
```bash
CREATE USER 'root'@'127.0.0.1' IDENTIFIED BY 'root-password';
CREATE USER 'root'@'::1' IDENTIFIED BY 'root-password';
CREATE USER 'root'@'172.17.0.1' IDENTIFIED BY 'root-password';
```

4. Create user `uaas`
```bash
mysql> CREATE USER 'uaas'@'172.17.0.1' IDENTIFIED BY 'uaas-password';
Query OK, 0 rows affected (0.01 sec)
```

5. Create database `uaas_db` and set permissions

``` bash
mysql> create database uaas_db;
Query OK, 1 row affected (0.00 sec)

mysql> GRANT ALL PRIVILEGES on uaas_db.* to 'uaas'@'172.17.0.1';
Query OK, 0 rows affected (0.00 sec)
```
``` bash
CREATE USER 'uaas'@'::1' IDENTIFIED BY 'uaas-password';
CREATE USER 'uaas'@'172.17.0.1' IDENTIFIED BY 'uaas-password';
```



6. Optional For `mainnet`
Create user 'maas'

```bash
mysql> CREATE USER 'maas'@'172.17.0.1' IDENTIFIED BY 'maas-password';
Query OK, 0 rows affected (0.01 sec)
```
``` bash
CREATE USER 'maas'@'::1' IDENTIFIED BY 'maas-password';
CREATE USER 'maas'@'172.17.0.1' IDENTIFIED BY 'maas-password';
```

Create database 'main_uaas_db'
``` bash
mysql> create database main_uaas_db;
Query OK, 1 row affected (0.00 sec)

mysql> GRANT ALL PRIVILEGES on main_uaas_db.* to 'maas'@'172.17.0.1';
Query OK, 0 rows affected (0.00 sec)
```



## Step 3 - Test Connection
1. Install the mysql-client on your local machine.
Should be able to execute the following on the command line:

```bash
mysqladmin -h localhost -P3306 --protocol=tcp -u root -p version
```

## Stoping and Starting MySQL
The Docker container will persist the data between sessions if correctly shutdown and started, using the following commands.

To stop the MySQL database in the docker container:
```bash
docker stop my-sql
```

To start the MySQL database in the docker container:
```bash
docker start my-sql
```

## Docker Compose MariaDB tuning

`docker-compose up` mounts `docker/mariadb/99-uaas.cnf` into the MariaDB container. It sets InnoDB options tuned for what used to be the indexer's write pattern; with the indexer on PostgreSQL only the Python REST API's reads remain, so these are now over-provisioned rather than wrong:

* `innodb_buffer_pool_size = 512M` — increase on dedicated hosts (typically 50–70% of RAM)
* `innodb_flush_log_at_trx_commit = 2` — faster writes with a small durability trade-off (appropriate for an indexer)
* `innodb_io_capacity` — raised for SSD-backed storage

Store `./data/mysql` on local SSD rather than network storage when possible.

For production, consider also raising `innodb_log_file_size` (requires a fresh datadir or removing `ib_logfile*` while MariaDB is stopped).

# MySQL Workbench (Optional)
MySQL Workbench provides a simple GUI for browsing the database.

To download MySQL Workbench use the following link https://dev.mysql.com/downloads/workbench/