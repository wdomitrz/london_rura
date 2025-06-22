const stationSelect = document.getElementById("stationSelect");
const departuresDiv = document.getElementById("departures");

let refreshIntervalId = null;

function getQueryParam(param) {
  const urlParams = new URLSearchParams(window.location.search);
  return urlParams.get(param);
}

function updateQueryParam(param, value) {
  const url = new URL(window.location);
  url.searchParams.set(param, value);
  window.history.replaceState({}, "", url);
}

async function fetchStations() {
  const modes = [
    "overground",
    "tube",
    "elizabeth-line",
    "dlr",
    // "bus",
    // "tram",
    // "river-bus",
    // "coach",
    // "national-rail"
  ];
  const modesParam = modes.join(",");
  const res = await fetch(`https://api.tfl.gov.uk/StopPoint/Mode/${modesParam}`);
  const data = await res.json();

  const filtered = data.stopPoints.filter(sp =>
    sp.commonName &&
    [
      "NaptanMetroStation",
      "NaptanRailStation",
      "NaptanBusCoachStopCluster",
      "NaptanOnstreetBusCoachStopPair",
      "TransportInterchange"
    ].includes(sp.stopType) &&
    sp.modes.some(mode => modes.includes(mode)) // <-- filter by requested modes
  );

  const uniqueStationsMap = new Map();
  for (const station of filtered) {
    if (!uniqueStationsMap.has(station.commonName)) {
      uniqueStationsMap.set(station.commonName, station);
    }
  }

  const stations = Array.from(uniqueStationsMap.values())
    .sort((a, b) => a.commonName.localeCompare(b.commonName));

  stationSelect.innerHTML = "";

  stations.forEach(station => {
    const opt = document.createElement("option");
    opt.value = station.id;
    const filteredModes = station.modes.filter(m => modes.includes(m));
    opt.textContent = `${station.commonName}${filteredModes.length ? ` (${filteredModes.join(", ")})` : ""}`;
    stationSelect.appendChild(opt);
  });

  const stationFromURL = getQueryParam("station");
  if (stationFromURL) {
    stationSelect.value = stationFromURL;
    stationSelect.dispatchEvent(new Event("change"));
  }
}


async function loadDepartures(stationId) {
  if (!stationId) {
    departuresDiv.innerHTML = "<p>Please select a station.</p>";
    return;
  }

  departuresDiv.innerHTML = "<p>Loading departures...</p>";

  try {
    const res = await fetch(`https://api.tfl.gov.uk/StopPoint/${stationId}/Arrivals`);
    const data = await res.json();

    const filtered = data
      .filter(dep => dep.destinationName)
      .sort((a, b) => a.timeToStation - b.timeToStation);

    if (filtered.length === 0) {
      departuresDiv.innerHTML = "<p>No upcoming departures found.</p>";
      return;
    }

    const groupedByPlatform = {};
    for (const dep of filtered) {
      const platform = dep.platformName || "Unknown Platform";
      if (!groupedByPlatform[platform]) {
        groupedByPlatform[platform] = [];
      }
      groupedByPlatform[platform].push(dep);
    }

    const sortedPlatforms = Object.keys(groupedByPlatform).sort((a, b) => {
      const numA = parseInt(a.match(/\d+/)) || 0;
      const numB = parseInt(b.match(/\d+/)) || 0;
      return numA - numB;
    });

    let html = "<h2>Departures by Platform</h2>";
    for (const platform of sortedPlatforms) {
      html += `
        <h3>${platform}</h3>
        <table>
          <thead>
            <tr>
              <th>Line</th>
              <th>Destination</th>
              <th>Arrival (min)</th>
            </tr>
          </thead>
          <tbody>
            ${groupedByPlatform[platform].slice(0, 10).map(dep => `
              <tr>
                <td>${dep.lineName}</td>
                <td>${dep.destinationName}</td>
                <td>${Math.round(dep.timeToStation / 60)}</td>
              </tr>
            `).join("")}
          </tbody>
        </table>
      `;
    }

    departuresDiv.innerHTML = html;

  } catch (error) {
    departuresDiv.innerHTML = "<p>Error fetching departures. Please try again later.</p>";
    console.error(error);
  }
}

stationSelect.addEventListener("change", () => {
  const stationId = stationSelect.value;
  if (!stationId) {
    departuresDiv.innerHTML = "<p>Please select a station.</p>";
    if (refreshIntervalId) {
      clearInterval(refreshIntervalId);
      refreshIntervalId = null;
    }
    updateQueryParam("station", "");
    return;
  }

  updateQueryParam("station", stationId);
  loadDepartures(stationId);
  if (refreshIntervalId) clearInterval(refreshIntervalId);
  refreshIntervalId = setInterval(() => {
    loadDepartures(stationId);
  }, 30000);
});

fetchStations();

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("service-worker.js");
}
