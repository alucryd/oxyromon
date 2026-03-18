import { GraphQLClient, gql } from "graphql-request";
import { purgingSystemId } from "./store.js";

const endpoint = import.meta.env.DEV ? "http://localhost:8000/graphql" : `${window.location.origin}/graphql`;
const graphQLClient = new GraphQLClient(endpoint);

export async function addToList(key, value, systemId = null) {
  const mutation = gql`
    mutation AddToList($key: String!, $value: String!, $systemId: Int) {
      addToList(key: $key, value: $value, systemId: $systemId)
    }
  `;

  const variables = { key, value, systemId };
  await graphQLClient.request(mutation, variables);
}

export async function removeFromList(key, value, systemId = null) {
  const mutation = gql`
    mutation RemoveFromList($key: String!, $value: String!, $systemId: Int) {
      removeFromList(key: $key, value: $value, systemId: $systemId)
    }
  `;

  const variables = { key, value, systemId };
  await graphQLClient.request(mutation, variables);
}

export async function setBool(key, value, systemId = null) {
  const mutation = gql`
    mutation SetBool($key: String!, $value: Boolean!, $systemId: Int) {
      setBool(key: $key, value: $value, systemId: $systemId)
    }
  `;

  const variables = { key, value, systemId };
  await graphQLClient.request(mutation, variables);
}

export async function setPreferRegions(value, systemId = null) {
  const mutation = gql`
    mutation SetPreferRegions($value: String!, $systemId: Int) {
      setPreferRegions(value: $value, systemId: $systemId)
    }
  `;

  const variables = { value, systemId };
  await graphQLClient.request(mutation, variables);
}

export async function setPreferVersions(value, systemId = null) {
  const mutation = gql`
    mutation SetPreferVersions($value: String!, $systemId: Int) {
      setPreferVersions(value: $value, systemId: $systemId)
    }
  `;

  const variables = { value, systemId };
  await graphQLClient.request(mutation, variables);
}

export async function setSubfolderScheme(key, value, systemId = null) {
  const mutation = gql`
    mutation SetSubfolderScheme($key: String!, $value: String!, $systemId: Int) {
      setSubfolderScheme(key: $key, value: $value, systemId: $systemId)
    }
  `;

  const variables = { key, value, systemId };
  await graphQLClient.request(mutation, variables);
}

export async function setDirectory(key, value, systemId = null) {
  const mutation = gql`
    mutation SetDirectory($key: String!, $value: String!, $systemId: Int) {
      setDirectory(key: $key, value: $value, systemId: $systemId)
    }
  `;

  const variables = {
    key,
    value,
    systemId,
  };
  await graphQLClient.request(mutation, variables);
}

export async function purgeSystem(systemId) {
  purgingSystemId.set(systemId);
  try {
    const mutation = gql`
      mutation PurgeSystem($systemId: Int!) {
        purgeSystem(systemId: $systemId)
      }
    `;

    const variables = {
      systemId,
    };
    await graphQLClient.request(mutation, variables);
  } finally {
    purgingSystemId.set(-1);
  }
}
