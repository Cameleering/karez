/*
 * A test OAuth validator module for PostgreSQL 18 (libpq/oauth.h). Test-only: it accepts the
 * token "karez-token-for-<role>" for the role that asks and nothing else. A real validator checks
 * a signature or asks an introspection endpoint; this one has no secrets and no network.
 */
#include "postgres.h"

#include <string.h>

#include "fmgr.h"
#include "libpq/oauth.h"

PG_MODULE_MAGIC;

static void
karez_startup(ValidatorModuleState *state)
{
}

static void
karez_shutdown(ValidatorModuleState *state)
{
}

static bool
karez_validate(const ValidatorModuleState *state, const char *token,
			   const char *role, ValidatorModuleResult *res)
{
	char		want[256];

	res->authorized = false;
	res->authn_id = NULL;

	snprintf(want, sizeof(want), "karez-token-for-%s", role);
	if (strcmp(token, want) == 0)
	{
		res->authorized = true;
		res->authn_id = pstrdup(role);
	}
	return true;
}

static const OAuthValidatorCallbacks callbacks = {
	PG_OAUTH_VALIDATOR_MAGIC,
	karez_startup,
	karez_shutdown,
	karez_validate,
};

const OAuthValidatorCallbacks *
_PG_oauth_validator_module_init(void)
{
	return &callbacks;
}
