import { invoke, isTauri } from '@tauri-apps/api/core';

export interface ArtifactLocation { id: string; url: string }
export const nativeArtifacts = isTauri() && !import.meta.env.DEV;
export const createNativeArtifact = (html: string) => invoke<ArtifactLocation>('create_artifact_preview', { html });
export const releaseNativeArtifact = (id: string) => invoke<void>('release_artifact_preview', { id });
