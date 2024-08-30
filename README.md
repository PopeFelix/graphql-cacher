# GraphQL Cacher

This is an application designed to run on the [Fastly](https://fastly.com/) [Compute@Edge](https://www.fastly.com/products/edge-compute/serverless) platform. Its purpose is to reduce the load on a [GraphQL](https://graphql.org/) backend by servicing requests via the Compute@Edge platform and the Fastly cache.

## NOTE

I developed this a while back and haven't gone through to see if everything still works properly. You have been warned.

## THEORY OF OPERATION

Consider a backend that uses GraphQL requests for all data access operations. These requests consist of one or more [Operations](https://spec.graphql.org/October2021/#sec-Language.Operations), which may be a query (i.e. a "read-only fetch"), a mutation (i.e. a write operation), or a subscription (i.e. "a long-lived request that fetches data in response to source events.")[^1]. These Operations contain a [Selection Set](https://spec.graphql.org/October2021/#sec-Selection-Sets), which is a collection of [Fields](https://spec.graphql.org/October2021/#Field), [Fragment Spreads](https://spec.graphql.org/October2021/#FragmentSpread), and [Inline Fragments](https://spec.graphql.org/October2021/#InlineFragment). Each Field and Fragment within a Selection Set may itself have a Selection Set associated with it. 

This application will break any GraphQL queries received (mutations and subscriptions are passed unmodified to the backend) down into a number of smaller queries - one for each Selection Set contained therein. Each of these subqueries will be individually sent as GET requests to the Picks GraphQL endpoint via the Fastly network. The responses will then be reassembled into a single JSON object and served to the requestor. As Fastly automatically cache GET requests, this allows the individual queries to be served from cache, reducing the load on the Picks backend and increasing read performance.

## Deployment

Execute `fastly compute publish` in the root of this repository (note that you must have the [Fastly CLI](https://developer.fastly.com/reference/cli/) installed). 

## Usage

Send a JSON encoded GraphQL request via POST to the configured application hostname. The path should be `/graphql`, e.g. `POST https://some-host-name.edgecompute.app/graphql`.

### Backend selection

The application currently defaults to the "QA" GraphQL backend. To select the production or "dev" backends, pass the header `X-Backend-Env` in your request:

| Backend | Value of `X-Backend-Env` header |
| ------- | ----------------------------- |
| Dev     | `dev`                         |
| QA      | `qa`                          |
| Prod    | `prod`                        |
 
[^1]: [GraphQL Specification, "Operations"](https://spec.graphql.org/October2021/#sec-Language.Operations)

## LICENSE AND COPYRIGHT

This software is copyright 2024 by Aurelia Peters.

GraphQL Cacher is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>. 

