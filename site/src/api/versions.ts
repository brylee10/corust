/**
 * API call to get the latest versions of rust and tools, if applicable.
 */

// Analogous to the correspondingly named Rust structs in the server
export interface Versions {
  stable: ChannelVersions;
  beta: ChannelVersions;
  nightly: ChannelVersions;
}

interface ChannelVersions {
  rustc: Version;
}

interface Version {
  release: string;
  commit_hash: string;
  commit_date: string;
}

export const getVersions = async (): Promise<Versions> => {
  const response = await fetch(
    `${process.env.REACT_APP_ENDPOINT_URI}/metadata/versions`
  );
  const data: Versions = await response.json();
  return data;
};
