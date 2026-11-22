# Geysir pricing engine

Geysir computes the price of an itinerary from base rates, seasonal multipliers and demand signals. Prices are recalculated every 15 minutes for the next 90 days of departures and cached.

Operators can set a margin floor; the default floor is 6% over cost and Geysir will never publish a price below it. Last-minute discounts of up to 20% are applied automatically to departures within 48 hours that are less than half full.

Rounding: prices in ISK are rounded to the nearest 100 kr, in EUR to the nearest 1 euro.
