#!/bin/bash

# Start container
docker run -it \
    -p 8081:8081 \
    --mount type=bind,source="$(pwd)"/data,target=/app/data \
    --rm uaas-service \
    $1 $2
