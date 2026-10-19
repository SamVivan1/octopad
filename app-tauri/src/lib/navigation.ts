/**
 * The four top-level pages of the app shell.
 *
 * Order is part of the contract: the sidebar renders these exactly in this
 * sequence (Buttons, Serial Console, Profiles, Settings) and `AppShell.test.ts`
 * pins it, so a new page has to be added here deliberately rather than slipping
 * into the navigation.
 */

export interface PageDefinition {
  /** Stable identifier, used for `data-page`, CSS hooks and deep links. */
  id: string;
  /** English label shown in the sidebar and as the page heading. */
  label: string;
}

export const PAGES: readonly PageDefinition[] = [
  { id: 'buttons', label: 'Buttons' },
  { id: 'console', label: 'Serial Console' },
  { id: 'profiles', label: 'Profiles' },
  { id: 'settings', label: 'Settings' },
] as const;

export type PageId = (typeof PAGES)[number]['id'];

/** The page the shell opens on. */
export const DEFAULT_PAGE: PageId = 'buttons';