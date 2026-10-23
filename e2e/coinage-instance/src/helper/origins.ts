/** The runtime origin, intended governance track and local account or transaction for a call. */
export type Origin = {
  requires: string;
  track: string;
  local: "sudo" | "eve" | "authorized";
};

export const ORIGINS = {
  /** Intended Asset Hub governance path. Local sudo supplies Root and XCM calls arrive on People as Root. */
  referendum: { requires: "Root", track: "technical_maintenance", local: "sudo" },
  /** `Coinage.create_sponsored_instance`: any signed account paying the creation deposit. */
  coinageSponsor: { requires: "Coinage.SponsorOrigin", track: "permissionless", local: "eve" },
  /** Example asset issuance. Eve owns and mints the local asset as a production issuer stand-in. */
  assetIssuer: { requires: "asset issuer (signed)", track: "permissionless", local: "eve" },
  /** Any provider holding both assets. Eve also issues the local example asset. */
  liquidityProvider: { requires: "signed (holds both assets)", track: "permissionless", local: "eve" },
  /** Unsigned authorized calls: chunk pages and the whitelisted subscribe. */
  authorized: { requires: "none", track: "permissionless", local: "authorized" },
} satisfies Record<string, Origin>;

/** An origin People dispatches itself. People has no `pallet_sudo`, so Root only reaches it as an
 * XCM `Transact` sent from Asset Hub. */
export type PeopleOrigin = Origin & { local: "eve" | "authorized" };
