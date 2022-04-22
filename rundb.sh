#!/bin/bash

# Start container
docker run --name=my-sql \
    --publish 3306:3306 \
    -d mysql/mysql-server \
    $1 $2