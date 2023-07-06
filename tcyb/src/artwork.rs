use crate::api;
use anyhow::bail;
use image::io::Reader as ImageReader;
use image::RgbaImage;
use log::{info, warn};
use std::io::Cursor;

pub async fn get_artwork(
    client_id: &str,
    client_secret: &str,
    names: &[String],
) -> anyhow::Result<()> {
    let access_token = api::get_tokens_by_client_credentials(client_id, client_secret).await?;
    let mut games: Vec<i64> = vec![];
    for name in names {
        let game_id = match game_ids(name, &access_token, client_id).await {
            Ok(r) => r,
            Err(e) => {
                warn!("{}", e);
                continue;
            }
        };
        games.push(game_id);
    }
    let artworks = api::get_cover(&access_token, client_id, &games).await?;
    let image_width: u32 = 1252;
    let mut img = RgbaImage::new(image_width, 704);
    for (idx, artwork) in artworks.iter().enumerate() {
        info!("{}: {}", artwork.game, artwork.image_id);
        let content = reqwest::Client::new()
            .get(format!(
                "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/{}.jpg",
                artwork.image_id
            ))
            .send()
            .await?
            .bytes()
            .await?;
        let on_top = ImageReader::new(Cursor::new(content))
            .with_guessed_format()?
            .decode()?;
        let len: u32 = artworks.len().try_into().unwrap();
        let index: u32 = idx.try_into().unwrap();
        let offset_x: u32 = (image_width / len) * index;
        image::imageops::overlay(&mut img, &on_top, offset_x.into(), 0);
    }
    img.save("thumbnails.jpg")?;
    Ok(())
}

async fn game_ids(name: &str, access_token: &str, client_id: &str) -> anyhow::Result<i64> {
    let games = api::search_game(access_token, client_id, name).await?;
    if games.is_empty() {
        bail!("Not found {}", name)
    }
    info!("{}: {}", games[0].game, games[0].name);
    Ok(games[0].game)
}
