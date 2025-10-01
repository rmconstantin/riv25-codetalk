#!/bin/bash

# Add DSQL permissions to a Lambda function
# Usage: ./add-dsql-permissions.sh <function-name>

if [ $# -eq 0 ]; then
    echo "Usage: $0 <function-name>"
    echo "Example: $0 ch03"
    exit 1
fi

FUNCTION_NAME=$1

echo "Adding DSQL permissions to function: $FUNCTION_NAME"

# Get the Lambda function's role name
ROLE_NAME=$(aws lambda get-function --function-name $FUNCTION_NAME --query 'Configuration.Role' --output text | cut -d'/' -f2)

if [ -z "$ROLE_NAME" ]; then
    echo "Error: Could not find role for function $FUNCTION_NAME"
    exit 1
fi

echo "Found IAM role: $ROLE_NAME"

# Create and attach a policy with DSQL permissions
aws iam put-role-policy --role-name $ROLE_NAME --policy-name DsqlAccess --policy-document '{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "dsql:DbConnect",
        "dsql:DbConnectAdmin"
      ],
      "Resource": "*"
    }
  ]
}'

if [ $? -eq 0 ]; then
    echo "Successfully added DSQL permissions to $FUNCTION_NAME"
else
    echo "Error: Failed to add DSQL permissions"
    exit 1
fi
