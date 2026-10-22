#!/bin/sh
set -eu

export AWS_ACCESS_KEY_ID="$S3_ACCESS_KEY" AWS_SECRET_ACCESS_KEY="$S3_SECRET_KEY"
export AWS_DEFAULT_REGION=us-east-1 AWS_MAX_ATTEMPTS=5 AWS_RETRY_MODE=adaptive

for file in "$@"
do
  sha256sum "$file" | cut -d' ' -f1 > "$file.sha256"
done

ls -l "$@"

for file in "$@"
do
  aws --endpoint-url "$S3_ENDPOINT" --cli-read-timeout 120 --cli-connect-timeout 30 s3 cp "$file" "s3://${S3_BUCKET}/${S3_PREFIX}/${CI_COMMIT_TAG}/$file"
  aws --endpoint-url "$S3_ENDPOINT" --cli-read-timeout 120 --cli-connect-timeout 30 s3 cp "$file.sha256" "s3://${S3_BUCKET}/${S3_PREFIX}/${CI_COMMIT_TAG}/$file.sha256"
done
