// Mirrors the SiteGraph API JSON shapes (see /openapi.json).

export type Role = "owner" | "editor" | "viewer";
export type Status = "draft" | "published";

export interface User {
  id: string;
  email: string;
  name: string;
}

export interface Membership {
  org_id: string;
  role: Role;
}

export interface FieldDef {
  key: string;
  label: string;
  type: string; // text | richtext | select | date | number | media | relation-ref
  required: boolean;
}

export interface ContentType {
  id: string;
  org_id: string;
  key: string;
  name: string;
  schema: { fields?: FieldDef[] };
}

export interface ContentObject {
  id: string;
  org_id: string;
  content_type_id: string;
  status: Status;
  data: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface Website {
  id: string;
  org_id: string;
  name: string;
  slug: string;
  domain?: string | null;
  created_at: string;
}

export interface ChannelAssignment {
  website_id: string;
  overrides?: Record<string, unknown> | null;
}

export interface ContentVersion {
  id: string;
  content_object_id: string;
  data: Record<string, unknown>;
  created_at: string;
}

export interface ApiToken {
  id: string;
  name: string;
  website_id: string | null;
}
