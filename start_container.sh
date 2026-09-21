#!/bin/bash
docker start a2o4 &>/dev/null || docker run --name a2o4 -v docker:/home/foo/library -p 2222:22 -d atmoz/sftp foo:pass:1001