export function shouldForceAccountSettings(input: {
  accounts: readonly unknown[]
  accountsQuerySucceeded: boolean
}): boolean {
  return input.accountsQuerySucceeded && input.accounts.length === 0
}
