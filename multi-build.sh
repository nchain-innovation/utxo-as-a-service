#!/bin/bash

# This build file creates the uaas web and rust services for use with the nChain rnd prototyping projects.
# For a faster build, you can use the cloud builder (cloud-nchain-rndprototyping).  
# Please check the allowed build minutes, as exceeding them may affect ability to build.
# Uncomment the --builder flag to enable the cloud builder, and comment out the --platform flag.


# Project Id1:  (uaas-web)
# Project Id2:  (uaas-rest)

# Tags
BASE_TAG1=uaas-web
VERSION1=v1.3
PUBLISH_TAG1=nchain/innovation-$BASE_TAG1:$VERSION1

# multi build, tag and push base images
# docker buildx build --builder cloud-nchain-rndprototyping --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG1 --file Python_Dockerfile .
docker buildx build  --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG1 --file Python_Dockerfile .

# Tags
BASE_TAG2=uaas-service
VERSION2=v1.2
PUBLISH_TAG2=nchain/innovation-$BASE_TAG2:$VERSION2

# multi build, tag and push base images
# docker buildx build  --builder cloud-nchain-rndprototyping --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG2 --file Rust_Dockerfile .
# docker buildx build  --platform linux/amd64,linux/arm64 --push -t $PUBLISH_TAG2 --file Rust_Dockerfile .