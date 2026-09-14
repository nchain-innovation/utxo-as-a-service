#!/bin/bash

# Start container
docker run -it \
    -p 5010:5010 \
    --mount type=bind,source="$(pwd)"/python/src,target=/app/python \
    --mount type=bind,source="$(pwd)"/data,target=/app/data \
    --network="bridge" \
    --rm uaas-web \
    $1 $2
