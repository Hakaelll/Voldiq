export function createNavigationRefresh() {
  let active: { owner: number; deferred: boolean } | null = null;
  return {
    begin(owner: number): void {
      active = { owner, deferred: false };
    },
    request(): boolean {
      if (active) {
        active.deferred = true;
        return false;
      }
      return true;
    },
    finish(owner: number): boolean {
      if (active?.owner !== owner) return false;
      const deferred = active.deferred;
      active = null;
      return deferred;
    },
    reset(): void {
      active = null;
    },
  };
}
