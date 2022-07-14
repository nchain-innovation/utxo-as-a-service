# Database

This section describes the commands to setup and run MySQL in a Docker image.

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

6. Optional For `mainnet`
Create user 'maas'

```bash
mysql> CREATE USER 'maas'@'172.17.0.1' IDENTIFIED BY 'maas-password';
Query OK, 0 rows affected (0.01 sec)
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

# MySQL Workbench (Optional)
MySQL Workbench provides a simple GUI for browsing the database.

To download MySQL Workbench use the following link https://dev.mysql.com/downloads/workbench/