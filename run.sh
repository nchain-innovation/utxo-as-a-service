#!/bin/bash

# Start container
docker run -it \
    --name="my-bnar" \
    --env BNAR_CONFIG='{"user_agent": "/Bitcoin SV:1.0.9/","ip": ["18.157.234.254",  "65.21.201.45" ], "port": 8333, "network": "Mainnet", "timeout_period": 60.0, "mysql_url": "mysql://bnar:bnar-password@host.docker.internal:3306/test_db"}' \
    --rm bnar \
    $1 $2