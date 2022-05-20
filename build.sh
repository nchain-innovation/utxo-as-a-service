#!/bin/bash

docker build --tag "uaas-web" --file Python_Dockerfile .

docker build --tag "uaas-service" --file Rust_Dockerfile .