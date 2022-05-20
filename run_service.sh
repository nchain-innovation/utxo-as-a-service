#!/bin/bash

# Start container
docker run -it \
    --mount type=bind,source="$(pwd)"/data,target=/app/data \
     -v /var/run/docker.sock:/var/run/docker.sock \
    --rm uaas-service \
    $1 $2
