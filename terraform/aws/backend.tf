# Remote state is recommended for shared/CI use. Defaults to local state so a
# fresh clone works without provisioning a bucket first. To enable, create an
# S3 bucket (+ optional DynamoDB lock table) and uncomment:
#
# terraform {
#   backend "s3" {
#     bucket         = "dabney-tfstate"
#     key            = "aws/apprunner/terraform.tfstate"
#     region         = "us-east-1"
#     dynamodb_table = "dabney-tfstate-lock"
#     encrypt        = true
#   }
# }
