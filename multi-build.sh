#!/bin/bash

# This build file creates the uaas web and rust services for use with the nchain rnd prototyping projects 

# Project Id1:  (uaas-web)
# Project Id2:  (uaas-rest)

# Tags
BASE_TAG1=uaas-web
VERSION1=v1.1
PUBLISH_TAG1=nchain/rnd-prototyping-$BASE_TAG1:$VERSION1

# multi build, tag and push base images
# docker buildx build --builder cloud-nchain-rndprototyping --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG1 --file Python_Dockerfile .
docker buildx build  --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG .

# Tags
BASE_TAG2=uaas-service
VERSION2=v1.1
PUBLISH_TAG2=nchain/rnd-prototyping-$BASE_TAG2:$VERSION2

# multi build, tag and push base images
# docker buildx build  --builder cloud-nchain-rndprototyping --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG2 --file Rust_Dockerfile .
docker buildx build  --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG .