#!/bin/sh

set -e

export VAULT_TOKEN=$(vault login -method=aws -token-only role=$VAULT_ROLE)

export TODOIST_TOKEN=$(vault read -field=token $VAULT_SECRETS_BASE/todoist)
export GITHUB_TOKEN=$(vault read -field=token $VAULT_SECRETS_BASE/github)

unset VAULT_TOKEN

reviewist_migrate
exec reviewist
