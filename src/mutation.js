import { GraphQLClient, gql } from "graphql-request";

const endpoint = "/graphql";
const graphQLClient = new GraphQLClient(endpoint);

export async function addToList(key, value) {
  const mutation = gql`
    mutation AddToList($key: String!, $value: String!) {
      addToList(key: $key, value: $value)
    }
  `;

  const variables = {
    key,
    value,
  };
  await graphQLClient.request(mutation, variables);
}

export async function removeFromList(key, value) {
  const mutation = gql`
    mutation RemoveFromList($key: String!, $value: String!) {
      removeFromList(key: $key, value: $value)
    }
  `;

  const variables = {
    key,
    value,
  };
  await graphQLClient.request(mutation, variables);
}

export async function setBool(key, value) {
  const mutation = gql`
    mutation SetBool($key: String!, $value: Boolean!) {
      setBool(key: $key, value: $value)
    }
  `;

  const variables = {
    key,
    value,
  };
  await graphQLClient.request(mutation, variables);
}

export async function setPreferRegions(value) {
  const mutation = gql`
    mutation SetPreferRegions($value: String!) {
      setPreferRegions(value: $value)
    }
  `;

  const variables = {
    value,
  };
  await graphQLClient.request(mutation, variables);
}

export async function setPreferVersions(value) {
  const mutation = gql`
    mutation SetPreferVersions($value: String!) {
      setPreferVersions(value: $value)
    }
  `;

  const variables = {
    value,
  };
  await graphQLClient.request(mutation, variables);
}
