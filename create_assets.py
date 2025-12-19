from PIL import Image, ImageDraw

def create_box(filename, color, opacity):
    # Create a 1920x300 box (lower third)
    # RGBA
    img = Image.new('RGBA', (1920, 300), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # Draw semi-transparent box
    r, g, b = color
    draw.rectangle([0, 0, 1920, 300], fill=(r, g, b, int(255 * opacity)))
    
    img.save(filename)

if __name__ == "__main__":
    create_box("/Users/hw24a094/nue/data/assets/caption_bg/simple_box.png", (0, 0, 0), 0.6)
    create_box("/Users/hw24a094/nue/data/assets/caption_bg/news_ticker.png", (0, 0, 150), 0.8)
    print("Assets created")
