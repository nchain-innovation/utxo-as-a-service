#!/bin/bash

# Start container
docker run -it \
    -p 5010:5010 \
    --mount type=bind,source="$(pwd)"/python/src,target=/app/src \
    --mount type=bind,source="$(pwd)"/data,target=/app/data \
    -v /var/run/docker.sock:/var/run/docker.sock \
    --rm uaas-web \
    $1 $2
