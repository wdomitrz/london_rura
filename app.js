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

function updatePageTitle(stationName) {
  if (stationName) {
    document.title = `London Rura - ${stationName}`;
  } else {
    document.title = "London Rura";
  }
}

const lineColors = {
  "Bakerloo": "#B36305",
  "Central": "#E32017",
  "Circle": "#FFD300",
  "District": "#00782A",
  "Elizabeth": "#6950a1",
  "Hammersmith & City": "#F3A9BB",
  "Jubilee": "#6A7278",
  "Metropolitan": "#9B0056",
  "Northern": "#000000",
  "Piccadilly": "#003688",
  "Victoria": "#00A0E2",
  "Waterloo & City": "#95CDBA",
};

async function fetchStations() {
  const res = await fetch("https://api.tfl.gov.uk/StopPoint/Mode/tube,elizabeth-line");
  const data = await res.json();
  const stations = data.stopPoints
    .filter(sp => sp.commonName && /(940GZZLU|940GZZCR)/.test(sp.id))
    .sort((a, b) => a.commonName.localeCompare(b.commonName));

  stations.forEach(station => {
    const opt = document.createElement("option");
    opt.value = station.id;
    opt.textContent = station.commonName;
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
            ${groupedByPlatform[platform].slice(0, 10).map(dep => {
        const color = lineColors[dep.lineName] || "#666";
        return `
                <tr>
                  <td style="color: ${color}; font-weight: bold;">${dep.lineName}</td>
                  <td>${dep.destinationName}</td>
                  <td>${Math.floor(dep.timeToStation / 60)}</td>
                </tr>
              `;
      }).join("")}
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
  const stationName = stationSelect.options[stationSelect.selectedIndex].text;

  if (!stationId) {
    departuresDiv.innerHTML = "<p>Please select a station.</p>";
    if (refreshIntervalId) {
      clearInterval(refreshIntervalId);
      refreshIntervalId = null;
    }
    updateQueryParam("station", "");
    updatePageTitle(null);
    return;
  }

  updateQueryParam("station", stationId);
  loadDepartures(stationId);
  updatePageTitle(stationName);

  if (refreshIntervalId) clearInterval(refreshIntervalId);
  refreshIntervalId = setInterval(() => {
    loadDepartures(stationId);
  }, 30000);
});

fetchStations();

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("service-worker.js");
}
