# Public API rate limits

Each API key may make 600 requests per minute, with a burst allowance of 50 requests in any 2-second window. Exceeding the limit returns HTTP 429 and a Retry-After header. Enterprise keys can be raised to 3,000 per minute on request. Search endpoints count double.
